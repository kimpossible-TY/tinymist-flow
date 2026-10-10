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
const visualState = (variants) =>
  Object.fromEntries(
    Object.entries(variants).map(([theme, variant]) => [
      theme,
      { page_hashes: variant.page_hashes, position: variant.position },
    ]),
  );
function assertResume(client, page = 120) {
  const resume = lastHint(client, "resume");
  assert.ok(resume, "new viewer must receive the saved edit");
  assert.equal(Number(resume.payload.toString().split(" ")[0]), page);
  assert.ok(
    client.messages.indexOf(resume) < client.messages.findIndex((x) => x.type === "new"),
    "resume precedes full frame",
  );
}

async function assertBodyEdit(clients, text, label) {
  for (const client of clients) client.messages.length = 0;
  await writeFile(passage, text);
  for (const client of clients) {
    await until(() => lastHint(client, "change"), label);
    const [page, x, y] = lastHint(client, "change").payload.toString().split(" ").map(Number);
    assert.equal(page, 120, label);
    assert.ok(x > 0 || y > 0, `${label}: must resolve source coordinates, not page fallback`);
    const hint = client.messages.indexOf(lastHint(client, "change"));
    await until(
      () => client.messages.slice(hint + 1).some((x) => x.type === "diff-v1"),
      `${label} delta`,
    );
  }
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
  const beforeComment = (await record()).variants;
  const unchanged = JSON.stringify(visualState(beforeComment));
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
    JSON.stringify(visualState((await record()).variants)),
    unchanged,
    "nonvisual edits must retain the saved position",
  );
  assert.notEqual(
    (await record()).variants.light.source_fingerprint,
    beforeComment.light.source_fingerprint,
    "a comment updates the source baseline even when the rendered output is unchanged",
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
    () =>
      resumedDark.messages
        .slice(latestRevision + 1)
        .some((x) => x.type === "new" || x.type === "diff-v1"),
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
  assertResume(await connect("dark"));
  assert.equal(
    lastHint(await connect("light"), "resume"),
    undefined,
    "a variant with stale source inputs must establish a new baseline after restart",
  );
  await stop();
  await writeFile(passage, "Edited while the service was stopped.\n");
  await start();
  const stale = await connect("light");
  assert.equal(
    lastHint(stale, "resume"),
    undefined,
    "restart with changed source must not guess an edit from the first changed page",
  );
  assert.equal((await record()).variants.light.position, null);
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
  const migratedLight = await connect("light");
  const migratedDark = await connect("dark");
  for (const client of [migratedLight, migratedDark]) {
    assert.equal(
      lastHint(client, "resume"),
      undefined,
      "legacy page hashes with a guessed page 2 must not restore an unverified edit",
    );
  }
  await delay(500);
  await assertBodyEdit(
    [migratedLight, migratedDark],
    "Live edit after legacy history migration.\n",
    "legacy history migration retains live source mapping",
  );
  assertResume(await connect("light"));
  assertResume(await connect("dark"));
  await stop();
  const upgraded = await record();
  for (const variant of Object.values(upgraded.variants)) {
    variant.page_hashes = variant.page_hashes.map(() => 0);
  }
  await writeFile(changeFile, JSON.stringify(upgraded));
  await start();
  assertResume(await connect("light"));
  assertResume(await connect("dark"));
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
  // An edited heading also changes its linked copy in the outline. The body
  // occurrence, rather than the first changed page, is the navigation target.
  const outlined = resolve(temp, "outlined.typ");
  await writeFile(
    outlined,
    (await readFile(input, "utf8"))
      .replace("= Last-edit restoration", "#outline(title: none)")
      .replace("[= Reading page #number]", "[Reading page #number]"),
  );
  await writeFile(passage, "= Original distant heading\n");
  const replacement = resolve(temp, "replacement.typ");
  await writeFile(replacement, "= Newly included distant heading\n");
  await start(outlined);
  const includedLight = await connect("light");
  const includedDark = await connect("dark");
  await until(async () => (await record())?.variants.dark, "include baselines");
  await delay(500);
  const outlinedSource = await readFile(outlined, "utf8");
  await writeFile(
    outlined,
    outlinedSource.replace('include "passage.typ"', 'include "replacement.typ"'),
  );
  for (const client of [includedLight, includedDark]) {
    await until(() => lastHint(client, "change"), "new include edit");
    assert.equal(
      Number(lastHint(client, "change").payload.toString().split(" ")[0]),
      120,
      "new dependencies must map their body text rather than the changed outline",
    );
  }
  await stop();
  await writeFile(outlined, outlinedSource);
  await start(outlined);
  const outlinedLight = await connect("light");
  const outlinedDark = await connect("dark");
  await until(async () => (await record())?.variants.dark, "outlined baselines");
  await delay(500);
  await writeFile(passage, "= Updated distant heading\n");
  for (const client of [outlinedLight, outlinedDark]) {
    await until(() => lastHint(client, "change"), "outlined heading edit");
    assert.equal(
      Number(lastHint(client, "change").payload.toString().split(" ")[0]),
      120,
      "heading edits must target the body, not the linked outline copy",
    );
    const hint = client.messages.indexOf(lastHint(client, "change"));
    await until(
      () => client.messages.slice(hint + 1).some((x) => x.type === "diff-v1"),
      "outlined edit delta",
    );
  }
  assertResume(await connect("light"));
  assertResume(await connect("dark"));
  const outlinedClients = [outlinedLight, outlinedDark];
  let body =
    "= Updated distant heading\n\nOriginal body text.\n\n$alpha$\n\n#figure(rect(width: 20pt, height: 10pt), caption: [Example])\n";
  await assertBodyEdit(outlinedClients, body, "body fixture setup");
  body = body.replace("Original body", "Updated body");
  await assertBodyEdit(outlinedClients, body, "ordinary paragraph edit");
  body = body.replace("$alpha$", "$beta$");
  await assertBodyEdit(outlinedClients, body, "math identifier edit");
  body = body.replace("width: 20pt", "width: 30pt");
  await assertBodyEdit(outlinedClients, body, "figure argument edit");
  await stop();
  const headered = resolve(temp, "headered.typ");
  await writeFile(
    headered,
    outlinedSource.replace(
      "#outline(title: none)",
      "#set page(header: context { query(heading).first().body })\n#outline(title: none)",
    ),
  );
  await writeFile(passage, "= Original repeated heading\n");
  await start(headered);
  const headeredLight = await connect("light");
  const headeredDark = await connect("dark");
  await until(async () => (await record())?.variants.dark, "header baselines");
  await delay(500);
  await assertBodyEdit(
    [headeredLight, headeredDark],
    "= Updated repeated heading\n",
    "heading copied into every running header",
  );
  await stop();
  const linked = resolve(temp, "linked.typ");
  await copyFile(input, linked);
  await writeFile(
    passage,
    "#link(<body-target>)[Original linked text]\n\nDestination <body-target>\n",
  );
  await start(linked);
  const linkedLight = await connect("light");
  const linkedDark = await connect("dark");
  await until(async () => (await record())?.variants.dark, "linked baselines");
  await delay(500);
  await writeFile(
    passage,
    "#link(<body-target>)[Updated linked text]\n\nDestination <body-target>\n",
  );
  for (const client of [linkedLight, linkedDark]) {
    await until(() => lastHint(client, "change"), "link-only edit");
    assert.equal(
      Number(lastHint(client, "change").payload.toString().split(" ")[0]),
      120,
      "authored internal-link text remains a valid navigation target",
    );
  }
  await stop();
  // Put the outline on page 2 and its heading body on page 120. An edit made
  // while stopped changes both, but neither occurrence is an observed edit.
  const stoppedOutline = resolve(temp, "stopped-outline.typ");
  await writeFile(
    stoppedOutline,
    outlinedSource
      .replace("#outline(title: none)", "[Cover page.]\n#pagebreak()\n#outline(title: none)")
      .replace("range(2, 131)", "range(3, 131)"),
  );
  await writeFile(passage, "= Original stopped heading\n");
  await start(stoppedOutline);
  const stoppedLight = await connect("light");
  const stoppedDark = await connect("dark");
  await delay(500);
  await assertBodyEdit(
    [stoppedLight, stoppedDark],
    "= Last observed heading edit\n",
    "observed heading before restart",
  );
  await stop();
  await writeFile(passage, "= Unobserved stopped heading edit\n");
  await start(stoppedOutline);
  for (const theme of ["light", "dark"]) {
    assert.equal(
      lastHint(await connect(theme), "resume"),
      undefined,
      "a stopped heading edit must not guess the outline on page 2",
    );
    assert.equal((await record()).variants[theme].position, null);
  }
  await stop();
  console.log(
    "PASS: initial baseline, ordered live edits in both themes, refresh/new viewer, disconnected edits, unchanged restart, stale-input baseline, legacy history migration, project isolation, new includes, outlined/repeated headings, paragraph/math/figure edits, authored internal links, and stopped outline edits without guessed navigation",
  );
  if (keep) {
    await writeFile(passage, "= Original distant heading\n");
    await start(outlined);
    await connect("light");
    await connect("dark");
    await delay(500);
    await writeFile(passage, "= Browser heading check on page 120\n");
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
