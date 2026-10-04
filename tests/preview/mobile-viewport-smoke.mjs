// Isolated browser regression: no live projects, ports, profiles or permissions.
// AGENT_BROWSER=/path/to/agent-browser node tests/preview/mobile-viewport-smoke.mjs /path/to/engine
import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { copyFile, mkdtemp, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { promisify } from "node:util";
import { setTimeout as delay } from "node:timers/promises";

const run = promisify(execFile);
const engine = process.argv[2];
const cli = process.env.AGENT_BROWSER;
assert.ok(engine && cli, "Pass engine path and AGENT_BROWSER executable");
const temp = await realpath(await mkdtemp(resolve(tmpdir(), "flow-mobile-viewport-")));
const session = `mobile-viewport-${process.pid}`;
await copyFile(
  new URL("../fixtures/preview-theme/main.typ", import.meta.url),
  resolve(temp, "main.typ"),
);
const child = spawn(
  engine,
  [
    "preview",
    resolve(temp, "main.typ"),
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
      TINYMIST_PREVIEW_CHANGE_FILE: resolve(temp, "changes.json"),
    },
    stdio: ["ignore", "pipe", "pipe"],
  },
);
let logs = "";
child.stdout.on("data", (x) => {
  logs += x;
});
child.stderr.on("data", (x) => {
  logs += x;
});
const browser = async (...args) => {
  const { stdout } = await run(cli, ["--session", session, "--json", ...args], {
    timeout: 45000,
    maxBuffer: 2e6,
  });
  const result = JSON.parse(stdout);
  assert.ok(result.success, stdout);
  return result.data;
};
const evaluate = async (code) => (await browser("eval", code)).result;
const settle = () =>
  browser(
    "wait",
    "--fn",
    `(() => {
  const d = document.getElementById('typst-container')?.documents?.[0]?.impl;
  return !!d && !d.isRendering && !d.patchQueue.length && d.vpTimeout === undefined &&
    !d.svgResizeAnchor && document.querySelectorAll('.typst-page-inner').length === 20;
})()`,
  );
const metrics = () =>
  evaluate(`(() => {
  const s = document.getElementById('typst-container-main');
  const p = document.querySelector('.typst-page-inner[data-page-number="9"]');
  const r = p.getBoundingClientRect(), v = s.getBoundingClientRect();
  return { width: v.width, height: v.height, pageWidth: r.width, pageLeft: r.left,
    pageFraction: (v.top-r.top)/r.height,
    scale: document.getElementById('typst-container').documents[0].impl.currentScaleRatio,
    theme: document.documentElement.dataset.documentTheme };
})()`);
const results = [];
try {
  let address;
  for (let n = 0; n < 300; n++) {
    address = /Data plane server listening on: (127\.0\.0\.1:\d+)/.exec(logs)?.[1];
    if (address) break;
    if (child.exitCode !== null) throw new Error(logs);
    await delay(100);
  }
  assert.ok(address, logs);
  await browser("open", `http://${address}`);
  await browser("snapshot", "-i");
  await browser("set", "viewport", "393", "852");
  await browser(
    "wait",
    "--fn",
    "Math.abs(document.querySelector('.typst-page-inner')?.getBoundingClientRect().width - 393) < 2",
  );
  await settle();
  await evaluate(`(() => {
    const s = document.getElementById('typst-container-main');
    const r = document.querySelector('.typst-page-inner[data-page-number="9"]').getBoundingClientRect();
    s.scrollTop += r.top - s.getBoundingClientRect().top + r.height * 0.25;
  })()`);
  await settle();
  const baseline = await metrics();
  assert.ok(Math.abs(baseline.pageFraction - 0.25) < 0.015, JSON.stringify(baseline));
  for (const [width, height] of [
    [852, 393],
    [852, 300],
    [393, 852],
    [1200, 800],
    [393, 700],
  ]) {
    await browser("set", "viewport", String(width), String(height));
    await browser(
      "wait",
      "--fn",
      `Math.abs(document.querySelector('.typst-page-inner').getBoundingClientRect().width - ${width}) < 2`,
    );
    await settle();
    const m = await metrics();
    assert.ok(Math.abs(m.width - width) < 2 && Math.abs(m.height - height) < 2, JSON.stringify(m));
    assert.ok(Math.abs(m.pageWidth - width) < 2 && Math.abs(m.pageLeft) < 2, JSON.stringify(m));
    assert.equal(m.scale, baseline.scale);
    assert.ok(
      Math.abs(m.pageFraction - baseline.pageFraction) < 0.015,
      JSON.stringify({ baseline, m }),
    );
    results.push(m);
  }
  // ResizeObserver must handle container-only changes, not only window.resize.
  await evaluate(`document.getElementById('typst-container').style.width = '350px'`);
  await browser(
    "wait",
    "--fn",
    "Math.abs(document.querySelector('.typst-page-inner').getBoundingClientRect().width - 350) < 2",
  );
  await settle();
  assert.ok(Math.abs((await metrics()).pageFraction - baseline.pageFraction) < 0.015);
  await evaluate(`document.getElementById('typst-container').style.removeProperty('width')`);
  await browser(
    "wait",
    "--fn",
    "Math.abs(document.querySelector('.typst-page-inner').getBoundingClientRect().width - 393) < 2",
  );
  await settle();
  await browser("snapshot", "-i");
  await browser("select", ".typst-theme-selector", "dark");
  await browser("wait", "--fn", "document.documentElement.dataset.documentTheme === 'dark'");
  await settle();
  const dark = await metrics();
  assert.ok(Math.abs(dark.pageFraction - baseline.pageFraction) < 0.015);
  // App zoom must survive rotation; native zoom is deliberately left enabled by meta.
  await evaluate(
    `(() => { const d = document.getElementById('typst-container').documents[0]; d.impl.currentScaleRatio = 1.5; d.addViewportChange(); })()`,
  );
  await browser(
    "wait",
    "--fn",
    "document.querySelector('.typst-page-inner').getBoundingClientRect().width > 580",
  );
  await settle();
  const zoomed = await metrics();
  await browser("set", "viewport", "852", "393");
  await browser(
    "wait",
    "--fn",
    "document.querySelector('.typst-page-inner').getBoundingClientRect().width > 1270",
  );
  await settle();
  const rotatedZoom = await metrics();
  assert.equal(rotatedZoom.scale, 1.5);
  assert.ok(Math.abs(rotatedZoom.pageFraction - zoomed.pageFraction) < 0.015);
  console.log(JSON.stringify({ baseline, rotations: results, dark, zoomed, rotatedZoom }, null, 2));
} finally {
  await browser("close").catch(() => {});
  child.kill("SIGTERM");
  if (child.exitCode === null) await new Promise((resolve) => child.once("exit", resolve));
  await rm(temp, { recursive: true, force: true });
}
