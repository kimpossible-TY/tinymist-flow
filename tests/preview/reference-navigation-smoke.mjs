// Isolated real-engine/browser regression; no managed profiles or user documents.
// AGENT_BROWSER=/path/to/agent-browser node tests/preview/reference-navigation-smoke.mjs /path/to/engine
// PREVIEW_FRONTEND_HTML=/path/to/built/index.html tests a new frontend with an existing engine.
import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { createServer, request } from "node:http";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { promisify } from "node:util";
import { setTimeout as delay } from "node:timers/promises";
import WebSocket from "ws";

const run = promisify(execFile);
const engine = process.argv[2],
  cli = process.env.AGENT_BROWSER;
assert.ok(engine && cli, "Pass engine path and AGENT_BROWSER executable");
const temp = await realpath(await mkdtemp(resolve(tmpdir(), "flow-reference-return-")));
const file = resolve(temp, "main.typ");
const session = `reference-return-${process.pid}`;
const source = `#let dark = sys.inputs.at("theme", default: "light") == "dark"
#set page(width: 240pt, height: 320pt, margin: 24pt, fill: if dark { rgb("17212f") } else { white })
#set text(size: 12pt, fill: if dark { white } else { black })
#set math.equation(numbering: "(1)")
#set heading(numbering: "1.")
${Array.from({ length: 20 }, (_, i) => {
  const page = i + 1;
  let body = `= Reading page ${page}\nA passage on page ${page}.`;
  if (page === 4)
    body +=
      '\n\nSee @equation-target and @figure-target.\n\n#link("https://example.com")[External reference]';
  if (page === 12)
    body += "\n\n$ x^2 + y^2 = z^2 $ <equation-target>\n\nContinue at @heading-target.";
  if (page === 15)
    body += "\n\n#figure(rect(width: 40pt, height: 25pt), caption: [Sample shape]) <figure-target>";
  if (page === 18) body += "\n\n== Last destination <heading-target>";
  return `${i ? "#pagebreak()\n" : ""}${body}`;
}).join("\n\n")}
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
      TINYMIST_PREVIEW_FOCUS_FILE: resolve(temp, "focus.json"),
      TINYMIST_PREVIEW_CHANGE_FILE: resolve(temp, "changes.json"),
    },
    stdio: ["ignore", "pipe", "pipe"],
  },
);
let logs = "",
  proxy,
  touchSocket;
child.stdout.on("data", (chunk) => {
  logs += chunk;
});
child.stderr.on("data", (chunk) => {
  logs += chunk;
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
const settle = async () => {
  await browser(
    "wait",
    "--fn",
    `(() => {
    const d = document.getElementById('typst-container')?.documents?.[0]?.impl;
    return !!d && !d.isRendering && !d.patchQueue.length && d.vpTimeout === undefined &&
      !d.svgResizeAnchor && document.querySelectorAll('.typst-page-inner').length === 20;
  })()`,
  );
  await browser(
    "wait",
    "--fn",
    `(() => {
    const s = document.getElementById('typst-container-main');
    const last = window.referenceScrollSample;
    const now = performance.now();
    if (!last || last.top !== s.scrollTop || last.left !== s.scrollLeft) {
      window.referenceScrollSample = {top:s.scrollTop,left:s.scrollLeft,at:now};
      return false;
    }
    return now - last.at > 200;
  })()`,
  );
  await evaluate("window.referenceScrollSample = undefined");
};
const moveToOrigin = async () => {
  await evaluate(`(() => {
    const s = document.getElementById('typst-container-main');
    const r = document.querySelector('.typst-page-inner[data-page-number="3"]').getBoundingClientRect();
    s.scrollTop += r.top - s.getBoundingClientRect().top + r.height * 0.07;
  })()`);
  await settle();
};
const position = (page = 3) =>
  evaluate(`(() => {
  const s = document.getElementById('typst-container-main');
  const p = document.querySelector('.typst-page-inner[data-page-number="${page}"]');
  const r = p.getBoundingClientRect(), v = s.getBoundingClientRect();
  return { top: s.scrollTop, left: s.scrollLeft,
    y: (v.top-r.top)/r.height, x: (v.left-r.left)/r.width,
    back: !document.querySelector('.typst-reference-back').hidden };
})()`);
const clickReference = async (page, index = 0) => {
  const result = await evaluate(`(() => {
    const page = document.querySelector('.typst-page[data-page-number="${page}"]');
    const links = [...page.querySelectorAll('[onclick]')].filter(e =>
      e.getAttribute('onclick').includes('handleTypstLocation'));
    const link = links[${index}];
    if (!link) return { links: links.map(e => e.outerHTML), page: page.outerHTML.slice(-2000) };
    link.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    return true;
  })()`);
  assert.equal(result, true, JSON.stringify(result));
  await settle();
};
const returnToOrigin = async (baseline) => {
  await browser("click", ".typst-reference-back");
  await settle();
  const restored = await position();
  assert.equal(restored.back, false, JSON.stringify(restored));
  assert.ok(Math.abs(restored.y - baseline.y) < 0.01, JSON.stringify({ baseline, restored }));
  assert.ok(Math.abs(restored.x - baseline.x) < 0.01, JSON.stringify({ baseline, restored }));
  assert.equal(
    await evaluate("document.getElementById('typst-container-main').style.overflowAnchor"),
    "",
  );
};

const enableTouch = async () => {
  touchSocket = new WebSocket((await browser("get", "cdp-url")).cdpUrl);
  await new Promise((done, reject) => {
    touchSocket.once("open", done);
    touchSocket.once("error", reject);
  });
  let id = 0;
  const command = (method, params = {}, sessionId) =>
    new Promise((done, reject) => {
      const current = ++id;
      const timeout = setTimeout(() => {
        touchSocket.off("message", response);
        reject(new Error(method));
      }, 10000);
      const response = (message) => {
        const result = JSON.parse(message.toString());
        if (result.id !== current) return;
        clearTimeout(timeout);
        touchSocket.off("message", response);
        if (result.error) reject(new Error(JSON.stringify(result.error)));
        else done(result.result);
      };
      touchSocket.on("message", response);
      touchSocket.send(JSON.stringify({ id: current, method, params, sessionId }));
    });
  const { targetInfos } = await command("Target.getTargets");
  const target = targetInfos.find(
    (target) => target.type === "page" && target.url.startsWith("http://127.0.0.1:"),
  );
  assert.ok(target);
  const { sessionId } = await command("Target.attachToTarget", {
    targetId: target.targetId,
    flatten: true,
  });
  await command(
    "Emulation.setTouchEmulationEnabled",
    { enabled: true, maxTouchPoints: 5 },
    sessionId,
  );
  return async ({ x, y }) => {
    await command(
      "Input.dispatchTouchEvent",
      { type: "touchStart", touchPoints: [{ x, y }] },
      sessionId,
    );
    await command("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] }, sessionId);
  };
};

try {
  let address;
  for (let n = 0; n < 300; n++) {
    address = /Data plane server listening on: (127\.0\.0\.1:\d+)/.exec(logs)?.[1];
    if (address) break;
    if (child.exitCode !== null) throw new Error(logs);
    await delay(100);
  }
  assert.ok(address, logs);
  let browserAddress = address;
  if (process.env.PREVIEW_FRONTEND_HTML) {
    const frontend = await readFile(process.env.PREVIEW_FRONTEND_HTML, "utf8");
    proxy = createServer((req, res) => {
      if (req.url === "/") {
        res.setHeader("Content-Type", "text/html");
        res.end(
          frontend
            .replaceAll("ws://127.0.0.1:23625", `ws://${browserAddress}`)
            .replaceAll("preview-arg:systemTheme:false", "preview-arg:systemTheme:true"),
        );
        return;
      }
      const upstream = request(
        `http://${address}${req.url}`,
        { headers: { ...req.headers, host: address } },
        (response) => {
          res.writeHead(response.statusCode, response.headers);
          response.pipe(res);
        },
      );
      upstream.on("error", () => {
        res.writeHead(502);
        res.end();
      });
      req.pipe(upstream);
    });
    proxy.on("upgrade", (req, socket, head) => {
      const upstream = request(`http://${address}${req.url}`, {
        headers: { ...req.headers, host: address, origin: `http://${address}` },
      });
      upstream.on("upgrade", (response, peer, peerHead) => {
        socket.write(
          `HTTP/1.1 101 Switching Protocols\r\n${Object.entries(response.headers)
            .map(([key, value]) => `${key}: ${value}`)
            .join("\r\n")}\r\n\r\n`,
        );
        if (head.length) peer.write(head);
        if (peerHead.length) socket.write(peerHead);
        socket.pipe(peer).pipe(socket);
        socket.on("error", () => peer.destroy());
        peer.on("error", () => socket.destroy());
        socket.on("close", () => peer.destroy());
      });
      upstream.on("error", () => socket.destroy());
      upstream.end();
    });
    await new Promise((done) => proxy.listen(0, "127.0.0.1", done));
    browserAddress = `127.0.0.1:${proxy.address().port}`;
  }
  await browser("open", `http://${browserAddress}`);
  await settle();
  await browser("select", ".typst-theme-selector", "light");
  await browser("wait", "--fn", "document.documentElement.dataset.documentTheme === 'light'");
  await settle();
  await moveToOrigin();
  const baseline = await position();
  assert.equal(baseline.back, false);
  await clickReference(3);
  assert.equal((await position()).back, true);
  assert.ok((await position()).top > baseline.top * 2);
  await returnToOrigin(baseline);

  // A figure reference uses the same path, and resize/zoom retain the passage.
  await clickReference(3, 1);
  await browser("set", "viewport", "393", "852");
  await settle();
  await evaluate(`(() => { const d = document.getElementById('typst-container').documents[0];
    d.impl.currentScaleRatio = 1.5; d.addViewportChange(); })()`);
  await settle();
  await returnToOrigin(baseline);

  // Following another link replaces the single return destination.
  await clickReference(3);
  const secondOrigin = await position(11);
  await clickReference(11);
  await browser("click", ".typst-reference-back");
  await settle();
  assert.ok(Math.abs((await position(11)).y - secondOrigin.y) < 0.01);
  assert.equal((await position()).back, false);

  // Editor/server navigation never creates a return action.
  await evaluate(`document.getElementById('typst-container').handleTypstLocation(
    document.querySelector('.typst-doc'), 4, 0, 70)`);
  await settle();
  assert.equal((await position()).back, false);
  await moveToOrigin();
  await clickReference(3);
  // A recent gesture defers the changed-page jump while a return is available.
  await evaluate(
    `document.getElementById('typst-container-main').dispatchEvent(new Event('wheel'))`,
  );
  await writeFile(file, source.replace("A passage on page 18.", "The edited passage on page 18."));
  await browser("wait", "--fn", "document.querySelector('.typst-change-jump')?.hidden === false");
  await settle();
  const buttons = await evaluate(`(() => {
    const a = document.querySelector('.typst-change-jump').getBoundingClientRect();
    const b = document.querySelector('.typst-reference-back').getBoundingClientRect();
    return { separate: a.bottom <= b.top || b.bottom <= a.top,
      inside: Math.max(a.right,b.right) <= innerWidth && Math.max(a.bottom,b.bottom) <= innerHeight };
  })()`);
  assert.deepEqual(buttons, { separate: true, inside: true });
  if (process.env.REFERENCE_SCREENSHOT)
    await browser("screenshot", process.env.REFERENCE_SCREENSHOT);
  await browser("click", ".typst-change-jump");
  await settle();
  assert.equal((await position()).back, true);
  await returnToOrigin(baseline);

  // Replacing the renderer removes the old origin and does not duplicate buttons.
  await clickReference(3);
  await browser("select", ".typst-theme-selector", "dark");
  await browser("wait", "--fn", "document.querySelector('.typst-reference-back')?.hidden === true");
  await settle();
  assert.equal((await position()).back, false);
  assert.equal(await evaluate("document.querySelectorAll('.typst-reference-back').length"), 1);
  await moveToOrigin();
  const dark = await position();
  await clickReference(3);
  await returnToOrigin(dark);

  // A native short tap passes through the mobile text mirror to the real SVG link.
  const tap = await enableTouch();
  await browser("reload");
  await settle();
  await moveToOrigin();
  assert.equal(await evaluate("matchMedia('(pointer: coarse)').matches"), true);
  const touchOrigin = await position();
  const linkPoint = await evaluate(`(() => {
    const link = document.querySelector('.typst-page[data-page-number="3"] [onclick]');
    const r = link.getBoundingClientRect();
    const x = (r.left+r.right)/2, y = (r.top+r.bottom)/2;
    return {x,y,mirror:!!document.elementFromPoint(x,y)?.closest('.typst-touch-selection')};
  })()`);
  assert.equal(linkPoint.mirror, true);
  await tap(linkPoint);
  await settle();
  assert.equal((await position()).back, true);
  const returnPoint = await evaluate(`(() => {
    const r = document.querySelector('.typst-reference-back').getBoundingClientRect();
    return {x:(r.left+r.right)/2,y:(r.top+r.bottom)/2};
  })()`);
  await tap(returnPoint);
  await settle();
  const touchReturned = await position();
  assert.equal(touchReturned.back, false);
  assert.ok(Math.abs(touchReturned.y - touchOrigin.y) < 0.01);
  assert.deepEqual((await browser("errors")).errors, []);
  console.log(
    JSON.stringify({
      desktop: baseline,
      mobileZoom: true,
      repeatedReference: true,
      independentChangeNavigation: true,
      actions: buttons,
      darkTheme: true,
      nativeTouchMirror: true,
    }),
  );
} catch (error) {
  console.error(logs.slice(-6000));
  console.error(
    await evaluate(`({ pages: document.querySelectorAll('.typst-page-inner').length,
    status: document.getElementById('typst-preview-status')?.textContent,
    documents: document.getElementById('typst-container')?.documents?.length,
    html: document.getElementById('typst-app')?.innerHTML.slice(0,500) })`).catch(() => undefined),
  );
  console.error(await browser("errors").catch(() => undefined));
  throw error;
} finally {
  touchSocket?.close();
  await browser("close").catch(() => {});
  proxy?.closeAllConnections();
  proxy?.close();
  child.kill("SIGTERM");
  if (child.exitCode === null) await new Promise((done) => child.once("exit", done));
  await rm(temp, { recursive: true, force: true });
}
