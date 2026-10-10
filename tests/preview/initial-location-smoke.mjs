// Real engine and headless browser; only disposable sources and loopback ports.
// PLAYWRIGHT_MODULE may point to an installed Playwright module for other hosts.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import WebSocket from "ws";

const root = resolve(import.meta.dirname, "../..");
const engine = process.argv[2] || resolve(root, "target/release/tinymist");
const { chromium } = await import(
  process.env.PLAYWRIGHT_MODULE ||
    resolve(
      homedir(),
      ".cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright/index.mjs",
    )
);
const temp = await realpath(await mkdtemp(resolve(tmpdir(), "tinymist-initial-location-")));
const input = resolve(temp, "main.typ");
const passage = resolve(temp, "passage.typ");
const changeFile = resolve(temp, "changes.json");
const source = [
  '#let dark = sys.inputs.at("theme", default: "light") == "dark"',
  '#set page(width: 240pt, height: 300pt, margin: 24pt, fill: if dark { rgb("#17212f") } else { white })',
  '#set text(size: 12pt, fill: if dark { rgb("#edf2f7") } else { rgb("#17212f") })',
  "Cover page.",
  "#pagebreak()",
  "#outline(title: none)",
  "#for number in range(3, 21) {",
  "  pagebreak()",
  "  [Reading page #number]",
  '  if number == 15 { include "passage.typ" } else { [An unchanged passage.] }',
  "}",
  "",
].join("\n");
let child, browser, address;
let logs = "";
const sockets = new Set();
const errors = [];
const pages = [];
const record = async () => JSON.parse(await readFile(changeFile, "utf8"));
async function until(predicate, label) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    const value = await predicate();
    if (value) return value;
    if (child?.exitCode !== null || child?.signalCode !== null) {
      throw new Error(`Preview exited: ${logs.slice(-4000)}`);
    }
    await delay(50);
  }
  throw new Error(`Timed out: ${label}\n${logs.slice(-4000)}`);
}
async function start() {
  logs = "";
  child = spawn(
    engine,
    [
      "preview",
      input,
      "--root",
      temp,
      "--no-open",
      "--follow-system-theme",
      "--partial-rendering=true",
      "--data-plane-host=127.0.0.1:0",
      "--control-plane-host=127.0.0.1:0",
    ],
    {
      env: {
        ...process.env,
        TINYMIST_ALLOWED_ORIGINS: "",
        TINYMIST_PREVIEW_FOCUS_FILE: resolve(temp, "focus.json"),
        TINYMIST_PREVIEW_CHANGE_FILE: changeFile,
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  child.stdout.on("data", (data) => (logs += data));
  child.stderr.on("data", (data) => (logs += data));
  child.on("error", (error) => (logs += error.message));
  address = await until(
    () => /Data plane server listening on: (127\.0\.0\.1:\d+)/.exec(logs)?.[1],
    "HTTP listener",
  );
}
async function stop() {
  for (const socket of sockets) socket.terminate();
  sockets.clear();
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise((done) => child.once("exit", done));
  child.kill("SIGINT");
  await Promise.race([exited, delay(5000)]);
  if (child.exitCode === null && child.signalCode === null) {
    child.kill("SIGKILL");
    await exited;
  }
}
async function baseline(theme) {
  const socket = new WebSocket(`ws://${address}/_theme/${theme}`, {
    origin: `http://${address}`,
  });
  sockets.add(socket);
  socket.on("error", () => {});
  let rendered = false;
  socket.on("message", (data) => {
    rendered ||= data.subarray(0, data.indexOf(44)).toString() === "new";
  });
  await until(() => socket.readyState === WebSocket.OPEN, `${theme} socket`);
  socket.send("current");
  await until(() => rendered, `${theme} baseline`);
}
const settle = (page) =>
  page.waitForFunction(
    () => {
      const doc = document.getElementById("typst-container")?.documents?.[0]?.impl;
      return (
        doc?.moduleInitialized &&
        !doc.isRendering &&
        !doc.patchQueue.length &&
        doc.vpTimeout === undefined &&
        !doc.svgResizeAnchor &&
        document.getElementById("typst-preview-status").hidden &&
        document.querySelectorAll(".typst-page-inner").length === 20 &&
        document.querySelectorAll(".tsel").length > 0
      );
    },
    null,
    { timeout: 60000 },
  );
const metrics = (page) =>
  page.evaluate(() => {
    const container = document.getElementById("typst-container-main");
    const viewport = container.getBoundingClientRect();
    const body = document
      .querySelector('.typst-page-inner[data-page-number="14"]')
      .getBoundingClientRect();
    return {
      top: container.scrollTop,
      scale: document.getElementById("typst-container").documents[0].impl.currentScaleRatio,
      bodyTop: body.top - viewport.top,
      bodyBottom: body.bottom - viewport.top,
      height: viewport.height,
      theme: document.documentElement.dataset.documentTheme,
      buttonHidden: document.querySelector(".typst-change-jump").hidden,
      socketOpens: window.testSocketOpens,
    };
  });
async function assertBodyVisible(page, label) {
  const result = await metrics(page);
  assert.ok(
    result.top > 1000 && result.bodyTop < result.height && result.bodyBottom > 0,
    `${label}: ${JSON.stringify(result)}`,
  );
  assert.equal(result.buttonHidden, true, label);
  return result;
}
const waitHint = (page, kind, count = 0) =>
  page.waitForFunction(
    ({ kind, count }) =>
      window.testSocketFrames.filter((frame) => frame.kind === kind).length > count,
    { kind, count },
  );
const hintCount = (page, kind) =>
  page.evaluate(
    (kind) => window.testSocketFrames.filter((frame) => frame.kind === kind).length,
    kind,
  );
async function assertBodyHint(page, kind) {
  const hint = await page.evaluate(
    (kind) => window.testSocketFrames.filter((frame) => frame.kind === kind).at(-1)?.payload,
    kind,
  );
  assert.ok(hint, `Missing ${kind} hint`);
  const [pageNumber, x, y] = hint.split(" ").map(Number);
  assert.equal(
    pageNumber,
    15,
    `Use the body on page 15 rather than its outline on page 2: ${hint}`,
  );
  assert.ok(x > 0 || y > 0, `Map the source coordinates: ${hint}`);
}
try {
  await writeFile(input, source);
  await writeFile(passage, "= Original distant heading\n");
  await start();
  await baseline("light");
  await baseline("dark");
  await stop();
  const legacy = await record();
  await writeFile(
    changeFile,
    JSON.stringify({
      schema_version: 1,
      project: legacy.project,
      variants: Object.fromEntries(
        Object.entries(legacy.variants).map(([theme, variant]) => [
          theme,
          { page_hashes: variant.page_hashes, position: [2, 0, 0] },
        ]),
      ),
    }),
  );
  await start();
  browser = await chromium.launch({
    headless: true,
    channel: process.env.PLAYWRIGHT_CHANNEL || "chrome",
  });
  for (const theme of ["light", "dark"]) {
    const page = await browser.newPage({
      viewport: { width: 393, height: 852 },
      colorScheme: theme,
    });
    pages.push(page);
    page.on("pageerror", (error) => errors.push(error.message));
    await page.addInitScript(() => {
      const Original = window.WebSocket;
      window.testSocketOpens = 0;
      window.testSocketFrames = [];
      window.WebSocket = class extends Original {
        constructor(...args) {
          super(...args);
          this.addEventListener("open", () => window.testSocketOpens++);
          this.addEventListener("message", (event) => {
            if (!(event.data instanceof ArrayBuffer)) return;
            const prefix = new TextDecoder().decode(new Uint8Array(event.data).subarray(0, 128));
            const comma = prefix.indexOf(",");
            const kind = prefix.slice(0, comma);
            window.testSocketFrames.push({
              kind,
              ...(["change", "resume"].includes(kind) ? { payload: prefix.slice(comma + 1) } : {}),
            });
          });
        }
      };
    });
    await page.goto(`http://${address}`, { waitUntil: "domcontentloaded" });
    await settle(page);
    const initial = await metrics(page);
    assert.equal(initial.theme, theme);
    assert.ok(
      initial.top < 2,
      `Legacy guessed page 2 moved a fresh ${theme} viewer: ${JSON.stringify(initial)}`,
    );
    assert.equal(initial.buttonHidden, true);
    assert.equal(await hintCount(page, "resume"), 0, "Legacy history must not supply an edit hint");
  }
  await delay(500); // Dependency subscription follows the first full render.
  await writeFile(passage, "= First observed distant heading\n");
  for (const page of pages) {
    await waitHint(page, "change");
    await settle(page);
    await assertBodyHint(page, "change");
    await assertBodyVisible(page, "Live edit navigates to its body");
    await page.reload({ waitUntil: "domcontentloaded" });
    await waitHint(page, "resume");
    await settle(page);
    await assertBodyHint(page, "resume");
    await assertBodyVisible(page, "Reload restores the actual last edit");
  }
  for (const page of pages) {
    await page.evaluate(() => (document.getElementById("typst-container-main").scrollTop = 0));
    await settle(page);
  }
  const changeCounts = await Promise.all(pages.map((page) => hintCount(page, "change")));
  for (const page of pages) {
    await page.dispatchEvent("#typst-container-main", "wheel", { deltaY: 0 });
  }
  await writeFile(passage, "= Deferred distant heading edit\n");
  for (const [index, page] of pages.entries()) {
    await waitHint(page, "change", changeCounts[index]);
    await page.locator(".typst-change-jump").waitFor({ state: "visible" });
    await settle(page);
    const deferred = await metrics(page);
    assert.ok(
      deferred.top < 2,
      `A reader gesture must defer the edit: ${JSON.stringify(deferred)}`,
    );
    await assertBodyHint(page, "change");
    await page.locator(".typst-change-jump").click();
    await settle(page);
    await assertBodyVisible(page, "Deferred button navigates to the body");
  }
  for (const page of pages) {
    await page.evaluate(() => {
      const doc = document.getElementById("typst-container").documents[0];
      doc.impl.currentScaleRatio = 1.25;
      doc.addViewportChange();
    });
    await settle(page);
    await page.evaluate(() => {
      const container = document.getElementById("typst-container-main");
      const reading = document.querySelector('.typst-page-inner[data-page-number="4"]');
      container.scrollTop +=
        reading.getBoundingClientRect().top - container.getBoundingClientRect().top;
    });
    await settle(page);
    const before = await metrics(page);
    assert.ok(before.top > 1000, `Reader moved away from the edited body: ${JSON.stringify(before)}`);
    assert.equal(before.scale, 1.25);
    await page.evaluate(() => document.getElementById("typst-container").typstWebsocket.close());
    await page.waitForFunction((count) => window.testSocketOpens > count, before.socketOpens);
    await settle(page);
    const reconnected = await metrics(page);
    assert.ok(Math.abs(reconnected.top - before.top) < 2, JSON.stringify({ before, reconnected }));
    assert.equal(reconnected.scale, before.scale);
    assert.equal(reconnected.buttonHidden, true);
    const theme = before.theme === "light" ? "dark" : "light";
    await page.locator(".typst-theme-selector").selectOption(theme);
    await page.waitForFunction(
      (theme) => document.documentElement.dataset.documentTheme === theme,
      theme,
    );
    await settle(page);
    const themed = await metrics(page);
    assert.ok(Math.abs(themed.top - before.top) < 2, JSON.stringify({ before, themed }));
    assert.equal(themed.scale, before.scale);
    assert.equal(themed.buttonHidden, true);
  }
  assert.deepEqual(errors, []);
  console.log(
    "PASS: legacy page 2 ignored in fresh light/dark viewers, mapped live edit and reload, gesture-deferred button, reconnect/theme reading position and zoom",
  );
} finally {
  await browser?.close().catch(() => {});
  await stop();
  await rm(temp, { recursive: true, force: true });
}
