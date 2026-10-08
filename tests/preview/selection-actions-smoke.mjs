// Isolated real-engine/browser checks; physical iPhone native gestures remain separate.
// AGENT_BROWSER=/path/to/agent-browser node tests/preview/selection-actions-smoke.mjs /path/to/engine
import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { promisify } from "node:util";
import { setTimeout as delay } from "node:timers/promises";
import WebSocket from "ws";

const run = promisify(execFile);
const engine = process.argv[2],
  cli = process.env.AGENT_BROWSER;
assert.ok(engine && cli, "Pass engine path and AGENT_BROWSER executable");
const temp = await realpath(await mkdtemp(resolve(tmpdir(), "flow-selection-actions-")));
const file = resolve(temp, "main.typ"),
  focusFile = resolve(temp, "focus.json");
const session = `selection-actions-${process.pid}`;
const brokenHelper = !!process.env.PREVIEW_BROKEN_HELPER;
const math = !!process.env.PREVIEW_MATH || brokenHelper;
if (math) {
  let helper = await readFile(new URL("./fixtures/highlighted.typ", import.meta.url), "utf8");
  if (brokenHelper)
    helper = helper.replace(
      'if body.has("children") { body.children } else { (body,) }',
      "body.children",
    );
  await writeFile(resolve(temp, "helpers.typ"), helper);
}
const source = `${math ? '#import "helpers.typ": highlighted\n#let scope(f) = f(none)\n' : ""}#set page(width: 320pt, height: 440pt, margin: 20pt)
#set text(size: 14pt, hyphenate: false)
#let dark = sys.inputs.at("theme", default: "light") == "dark"
#set page(fill: if dark { rgb("202020") } else { white })
#set text(fill: if dark { white } else { black })
The convergent power series is selected for review.

A second sentence for a red strike.
${math ? "#scope(s => ([Mixed formula $alpha + x$ ends here.]))\n\nStandalone $gamma$ equation.\n\nPartial $beta^2$ formula.\n\n#([Repeated range] * 2)\n" : ""}
${Array.from({ length: 19 }, (_, i) => `#pagebreak()\nAnother page ${i + 2} for virtualization.`).join("\n")}
`;
await writeFile(file, source);
const child = spawn(
  engine,
  [
    "preview",
    file,
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
      TINYMIST_PREVIEW_FOCUS_FILE: focusFile,
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
let touchSocket;
const emulateTouch = async (targetId) => {
  // The CLI's device preset changes metrics/UA but does not enable touch.
  // Keep this session attached so Chromium exposes the coarse-pointer path.
  touchSocket = new WebSocket((await browser("get", "cdp-url")).cdpUrl);
  await new Promise((resolve, reject) => {
    touchSocket.addEventListener("open", resolve, { once: true });
    touchSocket.addEventListener("error", reject, { once: true });
  });
  let id = 0;
  const command = (method, params, sessionId) =>
    new Promise((resolve, reject) => {
      const current = ++id;
      const timer = setTimeout(() => {
        touchSocket.removeEventListener("message", response);
        reject(new Error("CDP command timed out: " + method));
      }, 10000);
      const response = (event) => {
        const data = JSON.parse(event.data);
        if (data.id !== current) return;
        clearTimeout(timer);
        touchSocket.removeEventListener("message", response);
        if (data.error) reject(new Error(JSON.stringify(data.error)));
        else resolve(data.result);
      };
      touchSocket.addEventListener("message", response);
      touchSocket.send(JSON.stringify({ id: current, method, params, sessionId }));
    });
  const { sessionId } = await command("Target.attachToTarget", { targetId, flatten: true });
  await command(
    "Emulation.setTouchEmulationEnabled",
    { enabled: true, maxTouchPoints: 5 },
    sessionId,
  );
};
const wait = (code) => browser("wait", "--fn", code);
const settle = () =>
  wait(
    `(() => { const d = document.getElementById('typst-container')?.documents?.[0]?.impl; return !!d && !d.isRendering && !d.patchQueue.length && d.vpTimeout === undefined && !d.svgResizeAnchor; })()`,
  );
const select = async (quote, tail) => {
  await evaluate(`(() => {
    const q = ${JSON.stringify(quote)};
    const tail = ${JSON.stringify(tail) || "undefined"};
    const surface = document.querySelector('.typst-touch-selection') || document.getElementById('typst-app');
    const walker = document.createTreeWalker(surface, NodeFilter.SHOW_TEXT);
    let node;
    while (node = walker.nextNode()) {
      const index = node.textContent.indexOf(q);
      if (index < 0 || !node.parentElement.closest('.tsel,.typst-touch-selection-line')) continue;
      const range = document.createRange(); range.setStart(node, index); range.setEnd(node, index + q.length);
      if (tail) {
        do {
          const last = node.textContent.indexOf(tail);
          if (last >= 0) { range.setEnd(node, last + tail.length); break; }
        } while (node = walker.nextNode());
        if (!node) throw new Error('Missing range end: ' + tail);
      }
      const selected = document.getSelection(); selected.removeAllRanges(); selected.addRange(range);
      return true;
    }
    throw new Error('Text not populated: ' + q);
  })()`);
  await wait(
    "!document.querySelector('.typst-selection-actions').hidden && !document.querySelector('.typst-selection-actions button').disabled",
  );
};
const click = (label) => browser("find", "role", "button", "click", "--name", label, "--exact");
const marks = () => evaluate("document.querySelectorAll('.typst-review-marks span').length");
const focus = async () => JSON.parse(await readFile(focusFile, "utf8"));
const poll = async (predicate) => {
  for (let i = 0; i < 150; i++) {
    if (await predicate()) return;
    await delay(100);
  }
  throw new Error("Timed out\n" + logs.slice(-2000));
};

try {
  let address;
  await poll(() => {
    address = /Data plane server listening on: (127\.0\.0\.1:\d+)/.exec(logs)?.[1];
    return !!address;
  });
  if (process.env.PREVIEW_TOUCH) await browser("set", "device", "iPhone 15");
  const opened = await browser("open", `http://${address}`);
  await browser("set", "viewport", "393", "852");
  if (process.env.PREVIEW_TOUCH) {
    await emulateTouch(opened.targetId);
    await browser("reload");
  }
  await wait("document.querySelectorAll('.tsel').length > 0");
  if (process.env.PREVIEW_TOUCH) {
    await wait("document.querySelectorAll('.typst-touch-selection-line').length > 0");
    assert.equal(await evaluate("matchMedia('(pointer: coarse)').matches"), true);
    console.log("Mobile HTML selection layer populated");
  }
  await settle();
  await evaluate(
    `(() => { const sock = document.getElementById('typst-container').typstWebsocket; window.reviewRequests = []; const send = sock.send.bind(sock); sock.send = (message) => { window.reviewRequests.push(message); send(message); }; })()`,
  );
  if (brokenHelper) {
    const unchanged = await readFile(file, "utf8");
    await select("convergent power series");
    await click("Save for Codex");
    await poll(async () => (await focus()).selection?.text === "convergent power series");
    await select("convergent power series");
    await click("Highlight");
    await wait(
      "document.querySelector('.typst-focus-status').textContent.includes('Cannot safely map')",
    );
    assert.equal(await readFile(file, "utf8"), unchanged);
    console.log("Broken singleton helper leaves source unchanged and still permits Codex saving");
  } else {
    await select("convergent power series");
    await click("Save for Codex");
    await poll(async () => (await focus()).selection?.text === "convergent power series");
    const saved = await focus();
    assert.equal(saved.status, "selected");
    assert.equal(saved.selection.end.page_no, 1);
    await select("convergent", "review.");
    await click("Save for Codex");
    await poll(async () => (await focus()).selection?.text.includes("review."));
    const wrapped = await focus();
    assert.equal(
      wrapped.selection.text.replace(/\s+/g, " "),
      "convergent power series is selected for review.",
    );
    console.log("Exact selected text persisted with source context");
    const before = await readFile(file, "utf8");
    const requests = await evaluate("window.reviewRequests.length");
    const toolbar = await evaluate(`(() => {
    const rect = document.querySelector('.typst-selection-actions').getBoundingClientRect();
    const status = document.querySelector('.typst-focus-status').getBoundingClientRect();
    return { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom,
      height: rect.height, statusTop: status.top, viewport: innerWidth };
  })()`);
    assert.ok(toolbar.left >= 0 && toolbar.right <= toolbar.viewport);
    assert.ok(toolbar.top >= 0 && toolbar.bottom < toolbar.statusTop);
    assert.ok(toolbar.height <= 110, "Three actions fit compactly on the portrait viewport");
    if (process.env.SELECTION_SCREENSHOT)
      await browser(
        "screenshot",
        process.env.SELECTION_SCREENSHOT.replace(/(\.[^.]+)$/, "-toolbar$1"),
      );
    await click("Red strike");
    await wait("document.querySelectorAll('.typst-review-marks span').length > 0");
    assert.equal(await readFile(file, "utf8"), before);
    assert.equal(await evaluate("window.reviewRequests.length"), requests);
    assert.equal((await focus()).sequence, wrapped.sequence);
    await settle();
    const count = await marks();
    await browser("reload");
    await wait("document.querySelectorAll('.typst-review-marks span').length > 0");
    assert.equal(await marks(), count);
    if (!math) {
      await browser("snapshot", "-i");
      await browser("select", ".typst-theme-selector", "dark");
      await wait("document.documentElement.dataset.documentTheme === 'dark'");
      await settle();
      await wait(`document.querySelectorAll('.typst-review-marks span').length === ${count}`);
      assert.equal(await marks(), count);
      await browser("set", "viewport", "852", "393");
      await settle();
      await wait(`document.querySelectorAll('.typst-review-marks span').length === ${count}`);
      assert.equal(await marks(), count);
      await evaluate("document.getElementById('typst-container-main').scrollTop = 6000");
      await evaluate("document.getElementById('typst-container').documents[0].addViewportChange()");
      await settle();
      await wait(
        "document.querySelector('#typst-app .typst-page[data-page-number=\"0\"]').querySelectorAll('.tsel').length === 0",
      );
      await wait("document.querySelectorAll('.typst-review-marks span').length === 0");
      assert.equal(await marks(), 0);
      await evaluate("document.getElementById('typst-container-main').scrollTop = 0");
      await evaluate("document.getElementById('typst-container').documents[0].addViewportChange()");
      await settle();
      await wait("document.querySelectorAll('.typst-review-marks span').length > 0");
    }
    console.log(
      math
        ? "Red strike skips source writes/requests and survives refresh"
        : "Red strike skips source writes/requests and survives refresh, theme, resize and virtualization",
    );
    await click("Undo");
    assert.equal(await marks(), 0);
    await select("convergent power series");
    const beforePage = await evaluate(
      'document.querySelector("#typst-app .typst-page[data-page-number=\\\"0\\\"]").getAttribute("data-tid")',
    );
    await click("Highlight");
    await poll(async () =>
      (await readFile(file, "utf8")).includes(
        `${math ? "#highlighted" : "#highlight"}[convergent power series]`,
      ),
    );
    await settle();
    await wait(
      `document.querySelector('#typst-app .typst-page[data-page-number="0"]').getAttribute('data-tid') !== ${JSON.stringify(beforePage)}`,
    );
    await settle();
    console.log("Real Typst source highlight compiled");
    if (math) {
      const rejected = async (quote) => {
        const before = await readFile(file, "utf8");
        await select(quote);
        await click("Highlight");
        await wait(
          "document.querySelector('.typst-focus-status').textContent.includes('Cannot safely map')",
        );
        assert.equal(await readFile(file, "utf8"), before);
      };
      const mathCharacter = async (prefix) =>
        evaluate(`(() => {
      const surface = document.querySelector('.typst-touch-selection') || document.getElementById('typst-app');
      const text = Array.from(surface.querySelectorAll('.typst-touch-selection-line,.tsel')).map(x => x.textContent).join(' ');
      const prefix = ${JSON.stringify(prefix)};
      const index = text.indexOf(prefix);
      if (index < 0) return null;
      const codepoint = text.slice(index + prefix.length).trimStart().codePointAt(0);
      return codepoint === undefined ? null : String.fromCodePoint(codepoint);
    })()`);
      const beta = await mathCharacter("Partial");
      assert.ok(beta, "Partial equation is selectable");
      await rejected(beta);
      await rejected("Repeated range");
      const mixedPage = await evaluate(
        'document.querySelector("#typst-app .typst-page").getAttribute("data-tid")',
      );
      await select("Mixed", "ends here.");
      await click("Highlight");
      await poll(async () =>
        (await readFile(file, "utf8")).includes(
          "#highlighted[Mixed formula $alpha + x$ ends here.]",
        ),
      );
      await wait(
        `document.querySelector('#typst-app .typst-page').getAttribute('data-tid') !== ${JSON.stringify(mixedPage)}`,
      );
      await settle();
      const equationPage = await evaluate(
        'document.querySelector("#typst-app .typst-page").getAttribute("data-tid")',
      );
      const gamma = await mathCharacter("Standalone");
      assert.ok(gamma, "Singleton equation is selectable");
      await select(gamma);
      await click("Highlight");
      await poll(async () => (await readFile(file, "utf8")).includes("#highlighted[$gamma$]"));
      await wait(
        `document.querySelector('#typst-app .typst-page').getAttribute('data-tid') !== ${JSON.stringify(equationPage)}`,
      );
      await settle();
      console.log(
        "Math helper compiled mixed text/equations and singleton equations; partial math and repeated origins left source unchanged",
      );
    }
    await select("second sentence");
    await click("Red strike");
    await settle();
    assert.ok(await marks());
    await writeFile(
      file,
      (await readFile(file, "utf8")).replace("second sentence", "changed sentence"),
    );
    await wait("document.querySelectorAll('.typst-review-marks span').length === 0");
    await settle();
    console.log("Changed page invalidates temporary geometry");
    await select("changed sentence");
    await click("Save for Codex");
    await poll(async () => (await focus()).selection?.text === "changed sentence");
    const current = await focus();
    const stale = {
      ...current.position,
      revision: current.rendered_revision,
      selection: current.selection,
    };
    const changed = (await readFile(file, "utf8")).replace("changed sentence", "newer sentence");
    await writeFile(file, changed);
    await wait("document.querySelector('.typst-selection-actions button').disabled");
    await evaluate(
      `document.getElementById('typst-container').typstWebsocket.send('src-highlight ' + ${JSON.stringify(JSON.stringify(stale))})`,
    );
    await wait(
      "document.querySelector('.typst-focus-status').textContent.includes('Document changed')",
    );
    assert.equal(await readFile(file, "utf8"), changed);
    await evaluate(
      `document.getElementById('typst-container').typstWebsocket.send('src-point ' + ${JSON.stringify(JSON.stringify(stale))})`,
    );
    await poll(async () => (await focus()).status === "stale");
    assert.ok(!(await focus()).selection);
    await evaluate("document.getSelection().removeAllRanges()");
    await settle();
    console.log("Stale range requests do not edit source or retain current Codex text");
    await select("newer sentence");
    await click("Red strike");
    await settle();
    if (process.env.SELECTION_SCREENSHOT)
      await browser("screenshot", process.env.SELECTION_SCREENSHOT);
    await click("Clear marks");
    assert.equal(await marks(), 0);
    await settle();
    await evaluate(
      `document.getElementById('typst-container').onPreviewFocus(${JSON.stringify(current.position)})`,
    );
    await poll(async () => (await focus()).status === "selected" && !(await focus()).selection);
    console.log("Legacy tap focus still saves location without range data");
    assert.deepEqual((await browser("errors")).errors, []);
  }
} catch (error) {
  console.error(logs.slice(-5000));
  console.error(
    await evaluate(`(() => {
    const s = document.getSelection(), r = s?.rangeCount ? s.getRangeAt(0) : undefined;
    return { selection: s?.toString(), start: r?.startContainer.parentElement?.outerHTML.slice(0, 600),
      bounds: r ? Array.from(r.getClientRects()).map(x => ({x:x.x,y:x.y,width:x.width,height:x.height})) : [],
      pages: Array.from(document.querySelectorAll('.typst-page-inner')).slice(0,3).map(x => ({html:x.outerHTML, texts:x.parentElement.querySelectorAll('.tsel').length})),
      toolbar: document.querySelector('.typst-selection-actions')?.outerHTML,
      status: document.querySelector('.typst-focus-status')?.textContent,
      scroll: (() => { const m = document.getElementById('typst-container-main'); return {top:m.scrollTop, height:m.clientHeight, total:m.scrollHeight}; })(),
      partial: document.getElementById('typst-container').documents[0].impl.partialRendering,
      coarse: matchMedia('(pointer: coarse)').matches,
      texts: Array.from(document.querySelectorAll(".typst-touch-selection-line,.tsel")).map(x => x.textContent).slice(0,30),
      touchLines: document.querySelectorAll('.typst-touch-selection-line').length,
      firstPage: document.querySelector('#typst-app .typst-page[data-page-number="0"]')?.outerHTML.slice(0,600) };
  })()`).catch(() => ({})),
  );
  console.error(await browser("errors").catch(() => ({})));
  throw error;
} finally {
  touchSocket?.close();
  await browser("close").catch(() => {});
  child.kill("SIGTERM");
  if (child.exitCode === null) await new Promise((resolve) => child.once("exit", resolve));
  await rm(temp, { recursive: true, force: true });
}
