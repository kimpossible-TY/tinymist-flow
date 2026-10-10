// Only a temporary fixture and ephemeral listeners are used, never live projects.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import WebSocket from "ws";

const root = resolve(import.meta.dirname, "../..");
const engine = process.argv[2] || resolve(root, "target/release/tinymist");
const temp = await realpath(await mkdtemp(resolve(tmpdir(), "tinymist-git-focus-test-")));
const input = resolve(temp, "main.typ");
const earlier = resolve(temp, "z-earlier.typ");
const later = resolve(temp, "a-later.typ");
const changeFile = resolve(temp, "change.json");
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
function assertResume(client, page) {
  const resume = lastHint(client, "resume");
  assert.ok(resume, "new viewer must receive the saved edit");
  assert.equal(Number(resume.payload.toString().split(" ")[0]), page);
  assert.ok(
    client.messages.indexOf(resume) < client.messages.findIndex((x) => x.type === "new"),
    "resume precedes full frame",
  );
}

function git(...args) {
  return execFileSync(
    "git",
    [
      "-c",
      "core.hooksPath=/dev/null",
      "-c",
      "commit.gpgsign=false",
      "-c",
      "user.name=Test",
      "-c",
      "user.email=test@example.invalid",
      "-C",
      temp,
      ...args,
    ],
    { encoding: "utf8" },
  );
}
async function checkBoth(page) {
  for (const theme of ["light", "dark"]) {
    const client = await connect(theme);
    if (page === null) assert.equal(lastHint(client, "resume"), undefined);
    else assertResume(client, page);
  }
}
async function fresh() {
  await stop();
  await rm(changeFile, { force: true });
  await start();
}
try {
  git("init", "-q");
  await writeFile(resolve(temp, ".gitignore"), "change.json\nfocus.json\nignored.typ\n");
  await writeFile(
    input,
    `#set page(width: 400pt, height: 500pt)
Cover
#pagebreak()
#outline()
#pagebreak()
#include "z-earlier.typ"
#pagebreak()
Unchanged separator
#pagebreak()
#include "a-later.typ"
`,
  );
  await writeFile(earlier, "Original early body.\n");
  await writeFile(later, "= Original late heading\nLate body.\n");
  git("add", ".");
  git("commit", "-qm", "baseline");
  await writeFile(later, "= Changed late heading\nLate body.\n");
  git("add", "a-later.typ");
  await writeFile(earlier, "Changed early body.\n");
  const status = git("status", "--porcelain");
  await start();
  await checkBoth(3); // Body page 3 wins over alphabetical page 5 and its page-2 outline.
  assert.equal(git("status", "--porcelain"), status, "startup must not modify Git state");
  await delay(500);
  const clients = [await connect("light"), await connect("dark")];
  for (const client of clients) client.messages.length = 0;
  await writeFile(later, "= Latest live heading\nLate body.\n");
  for (const client of clients) {
    await until(() => lastHint(client, "change"), "live heading edit");
    assert.equal(Number(lastHint(client, "change").payload.toString().split(" ")[0]), 5);
  }
  await stop();
  await start();
  await checkBoth(5); // Saved live edit outranks the accumulated Git changes on page 3.
  await stop();
  await writeFile(later, "= Changed while stopped\nLate body.\n");
  await start();
  await checkBoth(3); // Invalidated history uses the earliest current Git candidate.
  await stop();
  git("add", "a-later.typ", "z-earlier.typ");
  git("commit", "-qm", "updated baseline");
  await fresh();
  await checkBoth(null); // Clean repository has no inferred edit.
  await stop();
  await writeFile(earlier, "Changed early body.\n// Comment only\n");
  await fresh();
  await checkBoth(null);
  await stop();
  git("checkout", "--", "z-earlier.typ");
  const placed = (word) => `#place(top + left, dy: 200pt)[Lower ${word}]
#place(top + left, dy: 20pt)[Upper ${word}]
`;
  await writeFile(earlier, placed("original"));
  git("add", "z-earlier.typ");
  git("commit", "-qm", "position fixture");
  await writeFile(earlier, placed("changed"));
  await fresh();
  await checkBoth(3);
  for (const variant of Object.values((await record()).variants)) {
    assert.ok(variant.position[2] < 100, "topmost visual position wins over source order");
  }
  await stop();
  await writeFile(earlier, "Before\n\nDeleted paragraph\n\nAfter\n");
  git("add", "z-earlier.typ");
  git("commit", "-qm", "deletion fixture");
  await writeFile(earlier, "Before\n\nAfter\n");
  await fresh();
  await checkBoth(3);
  await stop();
  git("rm", "--cached", "z-earlier.typ");
  git("commit", "-qm", "stop tracking included file");
  await writeFile(resolve(temp, "unrelated.typ"), "Unincluded changes must not navigate.\n");
  await writeFile(resolve(temp, "ignored.typ"), "Ignored changes must not navigate.\n");
  await fresh();
  await checkBoth(3); // An included untracked source is a candidate in its entirety.
  await stop();
  await rm(resolve(temp, ".git"), { recursive: true });
  git("init", "-q");
  await fresh();
  await checkBoth(1); // Unborn repository: the included main source starts on page 1.
  console.log(
    "PASS: earliest rendered page and topmost visual position, deletion mapping, outline exclusion, staged/unstaged changes, saved precedence, stale restart, clean/comment-only baseline, untracked dependencies, unborn repository, both themes and read-only Git state",
  );
} catch (error) {
  console.error(logs.slice(-10000));
  throw error;
} finally {
  await stop();
  await rm(temp, { recursive: true, force: true });
}
