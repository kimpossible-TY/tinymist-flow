// Real compiler and browser, disposable loopback fixture and transport gate only.
// AGENT_BROWSER=/path/to/agent-browser node tests/preview/activity-status-smoke.mjs /path/to/engine
import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { promisify } from "node:util";
import { setTimeout as delay } from "node:timers/promises";
import WebSocket, { WebSocketServer } from "ws";

const engine = process.argv[2],
  cli = process.env.AGENT_BROWSER;
assert.ok(engine && cli, "Pass engine path and AGENT_BROWSER executable");
const run = promisify(execFile);
const temp = await realpath(await mkdtemp(resolve(tmpdir(), "flow-preview-activity-")));
const file = resolve(temp, "main.typ"),
  session = `preview-activity-${process.pid}`;
const source =
  '#set page(width: 320pt, height: 440pt)\n#let theme = sys.inputs.at("theme", default: "light")\nActivity status fixture.\n';
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
let logs = "",
  address,
  proxyAddress,
  gate = true;
child.stdout.on("data", (x) => (logs += x));
child.stderr.on("data", (x) => (logs += x));
const pairs = new Set();
const proxy = createServer(async (_request, response) => {
  try {
    const upstream = await fetch(`http://${address}`);
    const html = (await upstream.text()).replaceAll(`ws://${address}`, `ws://${proxyAddress}`);
    response.writeHead(upstream.status, { "Content-Type": "text/html" });
    response.end(html);
  } catch (error) {
    response.writeHead(502);
    response.end(String(error));
  }
});
const wss = new WebSocketServer({ server: proxy });
wss.on("connection", (downstream, request) => {
  const upstream = new WebSocket(`ws://${address}${request.url}`, {
    headers: { Origin: `http://${address}` },
  });
  const pair = { downstream, upstream, queued: [] };
  pairs.add(pair);
  const requests = [];
  downstream.on("message", (data) => {
    if (upstream.readyState === WebSocket.OPEN) upstream.send(data.toString());
    else requests.push(data.toString());
  });
  upstream.on("open", () => requests.splice(0).forEach((data) => upstream.send(data)));
  upstream.on("message", (data, binary) => {
    const kind = data.subarray(0, data.indexOf(44)).toString();
    if (gate && ["new", "diff-v1"].includes(kind)) pair.queued.push([data, binary]);
    else if (downstream.readyState === WebSocket.OPEN) downstream.send(data, { binary });
  });
  downstream.on("close", () => {
    upstream.close();
    pairs.delete(pair);
  });
  upstream.on("close", () => downstream.close());
  upstream.on("error", () => downstream.close());
});
const flush = () => {
  gate = false;
  for (const pair of pairs)
    for (const [data, binary] of pair.queued.splice(0)) {
      pair.downstream.send(data, { binary });
    }
};
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
const wait = (code) => browser("wait", "--fn", code);
const phase = (name) =>
  wait(`document.getElementById('typst-preview-status')?.dataset.phase === '${name}'`);
const ready = () =>
  wait(
    "document.getElementById('typst-preview-status')?.hidden && document.querySelectorAll('.tsel').length > 0",
  );
try {
  for (let n = 0; n < 300; n++) {
    address = /Data plane server listening on: (127\.0\.0\.1:\d+)/.exec(logs)?.[1];
    if (address) break;
    if (child.exitCode !== null) throw new Error(logs);
    await delay(100);
  }
  assert.ok(address, logs);
  await new Promise((resolve) => proxy.listen(0, "127.0.0.1", resolve));
  proxyAddress = `127.0.0.1:${proxy.address().port}`;
  const html = await (await fetch(`http://${proxyAddress}`)).text();
  assert.match(html, /id="typst-preview-status"[^>]*role="status"/);
  assert.match(html, /prefers-reduced-motion/);
  await browser("open", `http://${proxyAddress}`);
  await browser("set", "viewport", "393", "852");
  await wait(
    "window.innerWidth === 393 && document.getElementById('typst-preview-status').getBoundingClientRect().right <= 393",
  );
  await phase("waiting");
  assert.equal(await evaluate("document.getElementById('typst-preview-status').hidden"), false);
  assert.equal(await evaluate("document.querySelectorAll('.tsel').length"), 0);
  const bounds = await evaluate(`(() => {
    const e = document.getElementById('typst-preview-status'), r = e.getBoundingClientRect();
    return { left: r.left, right: r.right, top: r.top, pointerEvents: getComputedStyle(e).pointerEvents };
  })()`);
  assert.ok(bounds.left >= 0 && bounds.right <= 393 && bounds.top > 40, JSON.stringify(bounds));
  assert.equal(bounds.pointerEvents, "none");
  await browser("screenshot", "/tmp/flow-preview-activity-waiting.png");
  flush();
  await ready();
  console.log("Initial static ring, waiting during held delivery and successful render passed");

  gate = true;
  await writeFile(file, source + "A live edit.\n");
  await phase("waiting");
  assert.equal(await evaluate("document.getElementById('typst-preview-status').hidden"), false);
  flush();
  await ready();
  console.log("Live compilation stays visible until the updated document renders");

  await writeFile(file, source + "#missing_activity_function()\n");
  await phase("compile-error");
  assert.equal(
    await evaluate("document.getElementById('typst-preview-status').dataset.busy"),
    "false",
  );
  await browser("reload");
  await phase("compile-error");
  await wait("document.querySelectorAll('.tsel').length > 0");
  await evaluate("document.getElementById('typst-container').documents[0].addViewportChange()");
  await delay(200);
  await phase("compile-error");
  console.log("Compiler error reaches late viewers and survives cached rendering/viewport work");

  await writeFile(file, source + "Recovered.\n");
  await ready();
  gate = true;
  await evaluate(`(() => {
    const d = document.getElementById('typst-container').documents[0].impl;
    const render = d.r.rerender;
    d.r.rerender = async () => { d.r.rerender = render; throw new Error('Injected activity smoke failure'); };
  })()`);
  await writeFile(file, source + "Renderer failure check.\n");
  await phase("waiting");
  flush();
  await phase("render-error");
  await evaluate("document.getElementById('typst-container').documents[0].addViewportChange()");
  await delay(200);
  await phase("render-error");

  gate = true;
  await evaluate("document.getElementById('typst-container').typstWebsocket.close()");
  await phase("reconnecting");
  await phase("waiting");
  flush();
  await ready();
  await browser("set", "viewport", "852", "393");
  assert.equal(await evaluate("document.getElementById('typst-preview-status').hidden"), true);
  console.log("Renderer error, reconnect and fresh render recovery passed");
} finally {
  await browser("close").catch(() => {});
  for (const { downstream, upstream } of pairs) {
    downstream.terminate();
    upstream.terminate();
  }
  wss.close();
  proxy.close();
  child.kill("SIGTERM");
  await Promise.race([new Promise((resolve) => child.once("exit", resolve)), delay(5000)]);
  if (child.exitCode === null) child.kill("SIGKILL");
  await rm(temp, { recursive: true, force: true });
}
