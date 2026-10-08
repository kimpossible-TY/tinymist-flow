// Run after build:preview and cargo build --release --bin tinymist.
// Only a temporary fixture is edited; live projects and ports are untouched.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import WebSocket from "ws";

const root = resolve(import.meta.dirname, "../..");
const engine = process.argv[2] || resolve(root, "target/release/tinymist");
const temp = await realpath(await mkdtemp(resolve(tmpdir(), "tinymist-theme-test-")));
const input = resolve(temp, "main.typ");
const focusFile = resolve(temp, "focus.json");
await copyFile(resolve(root, "tests/fixtures/preview-theme/main.typ"), input);
const child = spawn(
  engine,
  [
    "preview",
    input,
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
      TINYMIST_LOG: "tinymist=debug,tinymist_project=debug,tinymist_preview=debug",
      TINYMIST_PREVIEW_FOCUS_FILE: focusFile,
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
const sockets = [];
async function until(predicate, label) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    const value = await predicate();
    if (value) return value;
    if (child.exitCode !== null) throw new Error(`Preview exited: ${logs.slice(-4000)}`);
    await delay(50);
  }
  throw new Error(`Timed out: ${label}\n${logs.slice(-4000)}`);
}
function connect(url, origin) {
  const ws = new WebSocket(url, { origin });
  const messages = [];
  ws.on("message", (data) => {
    const comma = data.indexOf(44);
    messages.push({ type: data.subarray(0, comma).toString(), payload: data.subarray(comma + 1) });
  });
  ws.on("error", () => {});
  sockets.push(ws);
  return { ws, messages };
}
const frames = (client) => client.messages.filter((x) => x.type === "new" || x.type === "diff-v1");
const revision = (client) =>
  client.messages
    .filter((x) => x.type === "focus-revision")
    .at(-1)
    ?.payload.toString();
async function sourceTap(client, expectedLine) {
  for (let y = 25; y <= 42; y += 3) {
    const previous = client.messages.filter((x) => x.type === "focus").length;
    client.ws.send(
      `src-point ${JSON.stringify({ page_no: 1, x: 40, y, revision: revision(client) })}`,
    );
    const response = await until(
      () => client.messages.filter((x) => x.type === "focus")[previous],
      "focus response",
    );
    const result = JSON.parse(response.payload);
    if (result.status === "selected") {
      const record = JSON.parse(await readFile(focusFile, "utf8"));
      assert.equal(
        record.source.line,
        expectedLine,
        "tap must use the selected variant's source branch",
      );
      assert.equal(record.rendered_revision, revision(client));
      return record;
    }
  }
  throw new Error("Could not hit theme-specific text");
}

try {
  const address = await until(
    () => /Data plane server listening on: (127\.0\.0\.1:\d+)/.exec(logs)?.[1],
    "HTTP listener",
  );
  const origin = `http://${address}`;
  const html = await (await fetch(origin)).text();
  assert.ok(html.includes("preview-arg:systemTheme:true"));
  const light = connect(`ws://${address}/_theme/light`, origin);
  const dark = connect(`ws://${address}/_theme/dark`, origin);
  for (const client of [light, dark]) {
    await until(() => client.ws.readyState === WebSocket.OPEN, "WebSocket open");
    client.ws.send("current");
    await until(() => frames(client).length && revision(client), "compiled variant");
  }
  const digest = (client) =>
    createHash("sha256").update(frames(client).at(-1).payload).digest("hex");
  assert.notEqual(digest(light), digest(dark), "document variants must differ");
  const first = await sourceTap(light, 7);
  const second = await sourceTap(dark, 5);
  assert.ok(second.sequence > first.sequence, "focus record ordering must be shared");

  const counts = [frames(light).length, frames(dark).length];
  // The first frame precedes asynchronous dependency-watcher subscription.
  await delay(500);
  const source = await readFile(input, "utf8");
  await writeFile(input, source.replace("Theme following", "Live source update: theme following"));
  try {
    await until(
      () => frames(light).length > counts[0] && frames(dark).length > counts[1],
      "edit delivered to both viewers",
    );
  } catch (error) {
    console.error({
      before: counts,
      after: [frames(light).length, frames(dark).length],
      light: light.messages.map((x) => x.type),
      dark: dark.messages.map((x) => x.type),
    });
    throw error;
  }

  for (const [path, requestOrigin] of [
    ["/_theme/dark", "https://untrusted.example"],
    ["/_theme/unknown", origin],
  ]) {
    const rejected = connect(`ws://${address}${path}`, requestOrigin);
    await until(() => rejected.ws.readyState === WebSocket.CLOSED, "rejected connection");
    assert.equal(rejected.messages.length, 0);
  }
  console.log(
    "PASS: native variants, concurrent viewers, live edits, variant-correct ordered focus, origin rejection, and unknown routes",
  );
} finally {
  for (const ws of sockets) ws.terminate();
  const exited = new Promise((resolveExit) => child.once("exit", resolveExit));
  child.kill("SIGINT");
  await Promise.race([exited, delay(5000)]);
  if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  await rm(temp, { recursive: true, force: true });
}
