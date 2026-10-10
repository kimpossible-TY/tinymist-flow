// Verify abrupt viewer disconnects against a native engine on a temporary project.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { copyFile, mkdtemp, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import WebSocket from "ws";

const root = resolve(import.meta.dirname, "../..");
const engine = process.argv[2] || resolve(root, "target/release/tinymist");
const temp = await realpath(await mkdtemp(resolve(tmpdir(), "tinymist-disconnect-test-")));
const input = resolve(temp, "main.typ");
await copyFile(resolve(root, "tests/fixtures/preview-theme/main.typ"), input);
const child = spawn(
  engine,
  [
    "preview",
    input,
    "--root",
    temp,
    "--no-open",
    "--partial-rendering=true",
    "--follow-system-theme",
    "--data-plane-host=127.0.0.1:0",
    "--control-plane-host=127.0.0.1:0",
  ],
  {
    env: { ...process.env, TINYMIST_PREVIEW_FOCUS_FILE: resolve(temp, "focus.json") },
    stdio: ["ignore", "pipe", "pipe"],
  },
);
let logs = "";
child.stdout.on("data", (data) => {
  logs += data;
});
child.stderr.on("data", (data) => {
  logs += data;
});
const sockets = [];
async function until(predicate, label) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    assert.equal(child.exitCode, null, logs.slice(-4000));
    assert.equal(child.signalCode, null, logs.slice(-4000));
    const result = predicate();
    if (result) return result;
    await delay(20);
  }
  throw new Error(`Timed out: ${label}\n${logs.slice(-4000)}`);
}
function connect(address, theme) {
  const ws = new WebSocket(`ws://${address}/_theme/${theme}`, { origin: `http://${address}` });
  const messages = [];
  ws.on("error", () => {});
  ws.on("message", (data) => {
    const comma = data.indexOf(44);
    messages.push({ type: data.subarray(0, comma).toString(), bytes: data.length - comma - 1 });
  });
  sockets.push(ws);
  return { ws, messages };
}
async function current(client) {
  client.messages.length = 0;
  client.ws.send("current");
  await until(
    () => client.messages.some((message) => message.type === "new" && message.bytes > 1000),
    "complete document after disconnect",
  );
}

try {
  const address = await until(
    () => /Data plane server listening on: (127\.0\.0\.1:\d+)/.exec(logs)?.[1],
    "HTTP listener",
  );
  const retained = connect(address, "light");
  await until(() => retained.ws.readyState === WebSocket.OPEN, "retained viewer");
  await current(retained);

  // Reset sockets while configuration or requested document frames are still in flight.
  for (let batch = 0; batch < 8; batch++) {
    await Promise.all(
      Array.from(
        { length: 4 },
        (_, index) =>
          new Promise((resolveAbort) => {
            const client = connect(address, (batch + index) % 2 ? "dark" : "light");
            client.ws.once("open", () => {
              for (let request = 0; request < 8; request++) client.ws.send("current");
              client.ws.terminate();
            });
            client.ws.once("close", resolveAbort);
          }),
      ),
    );
    await delay(30);
    await current(retained);
  }

  for (const theme of ["light", "dark"]) {
    const client = connect(address, theme);
    await until(() => client.ws.readyState === WebSocket.OPEN, "reconnected viewer");
    await current(client);
    assert.ok(client.messages.some((message) => message.type === "focus-revision"));
    client.ws.close();
  }
  assert.ok(!logs.includes("panicked at"), logs.slice(-4000));
  console.log("PASS: 32 abrupt disconnects, retained viewer, reconnects and both-theme documents");
} finally {
  for (const socket of sockets) socket.terminate();
  child.kill("SIGTERM");
  if (child.exitCode === null && child.signalCode === null) {
    await new Promise((done) => child.once("exit", done));
  }
  await rm(temp, { recursive: true, force: true });
}
