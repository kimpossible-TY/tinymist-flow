// Only a temporary fixture and ephemeral listeners are used, never live projects.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { copyFile, mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import WebSocket from "ws";

const root = resolve(import.meta.dirname, "../..");
const engine = process.argv[2] || resolve(root, "target/release/tinymist");
const keep = process.argv.includes("--keep");
const temp = await realpath(await mkdtemp(resolve(tmpdir(), "tinymist-change-test-")));
const input = resolve(temp, "main.typ");
const passage = resolve(temp, "passage.typ");
const changeFile = resolve(temp, "change.json");
for (const name of ["main.typ", "passage.typ"]) {
  await copyFile(resolve(root, "tests/fixtures/preview-change", name), resolve(temp, name));
}
let child;
let logs = "";
let address;
const sockets = [];
async function until(predicate, label) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    const value = await predicate();
    if (value) return value;
    if (child?.exitCode !== null) throw new Error(`Preview exited: ${logs.slice(-4000)}`);
    await delay(50);
  }
  throw new Error(`Timed out: ${label}\n${logs.slice(-4000)}`);
}
async function start(entry = input) {
  logs = "";
  child = spawn(
    engine,
    [
      "preview",
      entry,
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
        TINYMIST_LOG: "tinymist_project::compiler=info,tinymist_preview=info",
        TINYMIST_PREVIEW_CHANGE_FILE: changeFile,
        TINYMIST_PREVIEW_FOCUS_FILE: resolve(temp, "focus.json"),
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  child.stdout.on("data", (x) => {
    logs += x;
  });
  child.stderr.on("data", (x) => {
    logs += x;
  });
  child.on("error", (error) => {
    logs += error.message;
  });
  address = await until(
    () => /Data plane server listening on: (127\.0\.0\.1:\d+)/.exec(logs)?.[1],
    "HTTP listener",
  );
}
async function stop() {
  for (const socket of sockets.splice(0)) socket.terminate();
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise((done) => child.once("exit", done));
  child.kill("SIGINT");
  await Promise.race([exited, delay(5000)]);
  if (child.exitCode === null && child.signalCode === null) {
    child.kill("SIGKILL");
    await exited;
  }
}
async function connect(theme) {
  const ws = new WebSocket(`ws://${address}/_theme/${theme}`, { origin: `http://${address}` });
  const messages = [];
  ws.on("message", (data) => {
    const comma = data.indexOf(44);
    messages.push({ type: data.subarray(0, comma).toString(), payload: data.subarray(comma + 1) });
  });
  ws.on("error", () => {});
  sockets.push(ws);
  await until(() => ws.readyState === WebSocket.OPEN, "WebSocket open");
  ws.send("current");
  await until(() => messages.some((x) => x.type === "new"), "full document");
  return { ws, messages };
}
const record = async () => {
  try {
    return JSON.parse(await readFile(changeFile, "utf8"));
  } catch {
    return undefined;
  }
};
const lastHint = (client, kind) => client.messages.filter((x) => x.type === kind).at(-1);
function assertResume(client, page = 120) {
  const resume = lastHint(client, "resume");
  assert.ok(resume, "new viewer must receive the saved edit");
  assert.equal(Number(resume.payload.toString().split(" ")[0]), page);
  assert.ok(
    client.messages.indexOf(resume) < client.messages.findIndex((x) => x.type === "new"),
    "resume precedes full frame",
  );
}

try {
  await start();
  const light = await connect("light");
  const dark = await connect("dark");
  assert.equal(lastHint(light, "resume"), undefined, "first baseline must not guess an edit");
  await until(async () => (await record())?.variants.dark, "both variant baselines");
  await delay(500); // Dependency watcher registration follows the first frame.
  await writeFile(passage, "First live edit on the distant page.\n");
  for (const client of [light, dark]) {
    await until(() => lastHint(client, "change"), "live edit hint");
    assert.equal(Number(lastHint(client, "change").payload.toString().split(" ")[0]), 120);
    const hint = client.messages.indexOf(lastHint(client, "change"));
    await until(
      () => client.messages.slice(hint + 1).some((x) => x.type === "diff-v1"),
      "ordered edit delta",
    );
  }
  const unchanged = JSON.stringify((await record()).variants);
  const revision = light.messages
    .filter((x) => x.type === "focus-revision")
    .at(-1)
    .payload.toString();
  await writeFile(passage, "First live edit on the distant page.\n// Nonvisual source edit.\n");
  await until(
    () =>
      light.messages
        .filter((x) => x.type === "focus-revision")
        .at(-1)
        ?.payload.toString() !== revision,
    "comment compilation",
  );
  assert.equal(
    JSON.stringify((await record()).variants),
    unchanged,
    "nonvisual edits must retain the saved position",
  );
  const fullFrames = light.messages.filter((x) => x.type === "new").length;
  assertResume(await connect("light"));
  await until(
    () => light.messages.filter((x) => x.type === "new").length > fullFrames,
    "existing viewer's full update",
  );
  assert.equal(
    lastHint(light, "resume"),
    undefined,
    "a new viewer must not navigate an existing reader",
  );
  const baseline = JSON.stringify((await record()).variants);
  const darkRevision = lastHint(dark, "focus-revision").payload.toString();
  for (const socket of sockets.splice(0)) socket.terminate();
  await delay(200);
  await writeFile(passage, "Edited with all viewers disconnected.\n");
  await delay(500);
  assert.equal(
    JSON.stringify((await record()).variants),
    baseline,
    "unviewed variants defer compiling and persisting edits until a viewer returns",
  );
  const resumedDark = await connect("dark");
  assertResume(resumedDark);
  await until(
    async () =>
      JSON.stringify((await record())?.variants.dark) !== JSON.stringify(JSON.parse(baseline).dark),
    "reconnected variant compiles and persists the offline edit",
  );
  await until(
    () => lastHint(resumedDark, "focus-revision")?.payload.toString() !== darkRevision,
    "offline edit revision delivered to reconnected viewer",
  );
  const latestRevision = resumedDark.messages.indexOf(lastHint(resumedDark, "focus-revision"));
  await until(
    () => resumedDark.messages.slice(latestRevision + 1).some((x) => x.type === "new" || x.type === "diff-v1"),
    "offline edit document follows its revision",
  );
  if (lastHint(resumedDark, "change")) {
    assert.equal(Number(lastHint(resumedDark, "change").payload.toString().split(" ")[0]), 120);
  }
  assert.equal(
    JSON.stringify((await record()).variants.light),
    JSON.stringify(JSON.parse(baseline).light),
    "still-unviewed light variant retains its previous successful baseline",
  );
  await stop();
  await start();
  assertResume(await connect("light"));
  await stop();
  await writeFile(passage, "Edited while the service was stopped.\n");
  await start();
  const stale = await connect("light");
  assertResume(stale);
  assert.equal(
    lastHint(stale, "resume").payload.toString(),
    "120 0 0",
    "restart must replace stale coordinates",
  );
  await stop();
  const other = resolve(temp, "other.typ");
  await copyFile(input, other);
  await start(other);
  assert.equal(
    lastHint(await connect("light"), "resume"),
    undefined,
    "another entry must not reuse this project's edit",
  );
  await stop();
  console.log(
    "PASS: initial baseline, ordered live edits in both themes, refresh/new viewer, disconnected edits, restart, stale output, and project isolation",
  );
  if (keep) {
    await start();
    await connect("light");
    await connect("dark");
    await delay(500);
    await writeFile(passage, "Browser restoration check on page 120.\n");
    await until(async () => {
      const saved = await record();
      return (
        saved?.variants.dark?.position?.[0] === 120 && saved?.variants.light?.position?.[0] === 120
      );
    }, "browser baseline");
    console.log(JSON.stringify({ url: `http://${address}/`, temp, passage, changeFile }));
    await new Promise((done) => {
      process.once("SIGINT", done);
      process.once("SIGTERM", done);
    });
  }
} finally {
  await stop();
  await rm(temp, { recursive: true, force: true });
}
