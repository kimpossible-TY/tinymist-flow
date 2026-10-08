#!/usr/bin/env python3
"""Repeat saved-book edits against isolated Tinymist LSP processes.

Only in-memory document copies are edited. Pass an isolated PDE book snapshot
with --book so unrelated live edits cannot change the comparison inputs.
This measures LSP/normal compilation under shared-machine load, not preview
network transport. Every edit uses a unique, intentionally missing font family
in a tiny text probe. Its successful-compilation warning proves that the newest
source reached compilation. Edit-result latency ends when that exact diagnostic
arrives, including diagnostic conversion and delivery; it is not pure CPU time.
Warmup also waits for a successful-compilation quiet window so initial dependency
watch enrollment can finish before measured edits begin. Query errors remain
failures; stabilization does not retry or discard measured requests.
Footprint is macOS process footprint, not resident memory.
"""

import argparse
import ctypes
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import queue
import re
import statistics
import subprocess
import sys
import threading
import time
import tempfile


BASE = Path(__file__).resolve().parent
REPO = BASE.parents[2]
spec = importlib.util.spec_from_file_location(
    "static_import_runner", REPO / "tests/perf/static-import-discovery/runner.py"
)
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class Usage(ctypes.Structure):
    _fields_ = [("uuid", ctypes.c_ubyte * 16), ("values", ctypes.c_uint64 * 18)]


LIBPROC = ctypes.CDLL("/usr/lib/libproc.dylib") if sys.platform == "darwin" else None


class Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


TIMEBASE = Timebase(1, 1)
if sys.platform == "darwin":
    if ctypes.CDLL("/usr/lib/libSystem.B.dylib").mach_timebase_info(ctypes.byref(TIMEBASE)) != 0:
        raise RuntimeError("cannot read the Mach CPU timebase")


def memory(pid):
    if LIBPROC is None:
        return {}
    usage = Usage()
    if LIBPROC.proc_pid_rusage(pid, 2, ctypes.byref(usage)) != 0:
        return {}
    return {
        "rss": usage.values[6], "footprint": usage.values[7],
        "user_mach_ticks": usage.values[0], "system_mach_ticks": usage.values[1],
        "user_ns": usage.values[0] * TIMEBASE.numer // TIMEBASE.denom,
        "system_ns": usage.values[1] * TIMEBASE.numer // TIMEBASE.denom,
    }


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False).encode()).hexdigest()


def sha256(path):
    hasher = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def source_manifest(roots):
    """Hash source/configuration inputs, excluding unrelated book videos/builds."""
    sources = {}
    for label, root in roots.items():
        if root is None or not root.exists():
            continue
        for path in sorted(root.rglob("*")):
            if path.is_file() and path.suffix.lower() in (".typ", ".toml", ".bib"):
                sources[f"{label}/{path.relative_to(root)}"] = sha256(path)
    return {"sha256": digest(sources), "file_count": len(sources), "files": sources}


def canonical(method, value):
    if method == "textDocument/semanticTokens/full" and isinstance(value, dict):
        return value.get("data")  # resultId is an opaque server-local identifier.
    if method == "textDocument/completion" and isinstance(value, dict):
        # Item order can differ because of interning; retain every item field.
        return {
            "isIncomplete": value.get("isIncomplete"),
            "items": sorted(value.get("items", []), key=lambda v: json.dumps(v, sort_keys=True)),
        }
    return value


def query_counts(server_info):
    counts = []
    for project in server_info.values():
        table = runner.StatsTable()
        table.feed(project["stats"]["global"])
        if not table.rows or table.rows[0][:2] != ["Name", "Count"]:
            raise ValueError("missing global query statistics")
        counts.append({row[0]: int(row[1]) for row in table.rows[1:] if len(row) >= 2})
    if not counts or any(count != counts[0] for count in counts[1:]):
        raise ValueError("missing or inconsistent global query counts")
    return {name: counts[0].get(name, 0) for name in ("analyze_expr", "analyze_expr_eval", "main_compile")}


PROBE_SLOT = "__FLOW_REPLAY_FONT__"
PROBE_PREFIX = "flow-replay-confirm-"

DURATION = re.compile(r"([0-9.]+)(ns|µs|us|ms|s)")


def seconds(text):
    match = DURATION.fullmatch(text.strip())
    if match is None:
        raise ValueError(f"unrecognized duration: {text!r}")
    return float(match[1]) * {"ns": 1e-9, "µs": 1e-6, "us": 1e-6, "ms": 1e-3, "s": 1}[match[2]]


def split_logs(path, output):
    groups = {name: [] for name in ("compile", "stages", "comemo", "source", "vfs", "legacy_cache", "sweep_queue", "query_queue")}
    timing = {name: [] for name in groups}
    for line in path.read_text(errors="replace").splitlines():
        kind = None
        for marker, name in (
            ("compilation succeeded", "compile"),
            ("compile timing project=", "stages"),
            ("evict comemo cache in ", "comemo"),
            ("evict source cache in ", "source"),
            ("evict VFS cache in ", "vfs"),
            ("evict cache in ", "legacy_cache"),
            ("automatic cache sweep queued for ", "sweep_queue"),
            ("QueryQueue: admitted revision ", "query_queue"),
        ):
            if marker in line:
                kind = name
                break
        if kind is None:
            continue
        groups[kind].append(line)
        separator = {"sweep_queue": " for ", "query_queue": " after "}.get(kind, " in ")
        tail = line.rsplit(separator, 1)[-1]
        found = DURATION.match(tail)
        if found:
            timing[kind].append(seconds(found[0]))
    for name, lines in groups.items():
        (output / f"{name}.log").write_text("\n".join(lines) + ("\n" if lines else ""))
    return timing


def fixture(root):
    entry = root / "main.typ"
    chapter = root / "chapter 3/3.4.typ"
    original = chapter.read_text()
    marker = "#local-tag-scope(s => [\n"
    position = original.index(marker) + len(marker)
    insertion = "\n#let __flow_probe = s.tag\n"
    # The fallback glyph keeps the probe a valid document. The unique missing
    # font produces a warning in this compilation's diagnostics without adding
    # a protocol query that could evaluate or compile the document itself.
    diagnostic_probe = f'#text(font: "{PROBE_SLOT}", size: 0.1pt)[.]\n'
    text = original[:position] + insertion + diagnostic_probe + original[position:]
    cursor = {
        "line": text[:position + len(insertion)].count("\n") - 1,
        "character": len("#let __flow_probe = s."),
    }
    heading = "== Classical evolution equations: constructing kernels"
    if text.count(heading) != 1:
        raise ValueError("expected exactly one replay heading")
    if text.splitlines()[cursor["line"]][cursor["character"]:] != "tag":
        raise ValueError("completion cursor is not immediately before tag")
    return entry, chapter, text, cursor, heading


class Session:
    def __init__(self, binary, root, config, output, result):
        self.output, self.result = output, result
        self.started = time.monotonic()
        self.client = CapturedClient(binary, root, config, output / "server.log", output / "wire.jsonl", self.started)
        self.pending, self.responses = {}, {}
        self.compile_status = None
        self.compile_successes = 0
        self.status_at = self.started
        self.compile_activity_at = self.started
        self.diagnostic_markers = {}
        self.events_path = (output / "events.jsonl").open("w")

    def event(self, name, **values):
        item = {"t": round(time.monotonic() - self.started, 6), "name": name,
                **memory(self.client.process.pid), **values}
        self.result["events"].append(item)
        line = json.dumps(item)
        self.events_path.write(line + "\n")
        self.events_path.flush()
        print(json.dumps({"mode": self.result["mode"], **item}), flush=True)

    def send(self, method, params, key):
        request_id = self.client.next_id
        self.client.next_id += 1
        self.pending[request_id] = {"key": key, "method": method, "sent": time.monotonic()}
        self.client.send({"id": request_id, "method": method, "params": params})
        return request_id

    def pump_one(self, timeout=0.1):
        try:
            received, message = self.client.messages.get(timeout=max(0, timeout))
        except queue.Empty:
            return
        if isinstance(message, Exception):
            raise message
        if "method" in message and "id" in message:
            self.client.answer_server_request(message)
        elif "id" in message:
            request_id = message["id"]
            pending = self.pending.pop(request_id, None)
            if pending is None:
                self.event("unexpected_response", response=message)
                self.result["unexpected_responses"].append(message)
                return
            elapsed = received - pending["sent"]
            key, method = pending["key"], pending["method"]
            response = {"key": key, "method": method, "seconds": elapsed,
                        "received_at": received - self.started, "message": message}
            self.responses[request_id] = response
            (self.output / "responses" / f"{key}.json").write_text(json.dumps(response, indent=2))
            value = message.get("result")
            summary = {"method": method, "seconds": elapsed, "error": message.get("error"),
                       "canonical_sha256": digest(canonical(method, value)) if "result" in message else None}
            if isinstance(value, dict):
                summary["size"] = len(value.get("items", value.get("data", [])))
            self.result["responses"][key] = summary
            self.event("response", key=key, request_id=request_id, **summary)
        elif message.get("method") == "tinymist/compileStatus":
            params = message.get("params", {})
            previous = self.compile_status
            self.compile_status = params.get("status")
            # Word-count updates resend the current compile status. They must
            # not move the time at which a status transition was observed.
            if self.compile_status != previous:
                self.status_at = received
            if self.compile_status != previous or self.compile_status == "compiling":
                self.compile_activity_at = received
            if self.compile_status == "compileSuccess" and previous == "compiling":
                self.compile_successes += 1
            self.event("compile_status", status=self.compile_status, page_count=params.get("pageCount"),
                       received_at=received - self.started)
        elif message.get("method") == "textDocument/publishDiagnostics":
            self.compile_activity_at = received
            params = message.get("params", {})
            uri = params.get("uri", "")
            diagnostics = params.get("diagnostics", [])
            self.result["diagnostics"][uri] = diagnostics
            for diagnostic in diagnostics:
                message_text = diagnostic.get("message", "")
                matched = re.fullmatch(r"unknown font family: (flow-replay-confirm-[a-z0-9-]+)", message_text)
                if matched and diagnostic.get("severity") == 2:
                    marker = matched[1]
                    key = (uri, marker)
                    if key not in self.diagnostic_markers:
                        self.diagnostic_markers[key] = received
                        self.event("compiled_marker", marker=marker, uri=uri,
                                   received_at=received - self.started)

    def pump(self, duration):
        end = time.monotonic() + duration
        while time.monotonic() < end:
            self.pump_one(min(0.1, end - time.monotonic()))

    def collect(self, ids, timeout):
        deadline = time.monotonic() + timeout
        while any(request_id not in self.responses for request_id in ids):
            if time.monotonic() >= deadline:
                missing = [self.pending.get(i) for i in ids if i not in self.responses]
                raise TimeoutError(f"pending requests after {timeout}s: {missing}")
            self.pump_one(min(0.1, deadline - time.monotonic()))
        return [self.responses[i] for i in ids]

    def request(self, method, params, key, timeout):
        response, = self.collect([self.send(method, params, key)], timeout)
        if "error" in response["message"]:
            raise RuntimeError(f"{key}: {response['message']['error']}")
        return response["message"].get("result")

    def settle(self, uri, marker, sent_at, timeout):
        """Await this exact source version's compiler diagnostic, not an old status.

        StatusAll has no document revision and word counts resend it, so status
        timestamps cannot prove completion of the latest edit in a burst. A
        unique marker warning comes from the actual completed compilation.
        """
        deadline = time.monotonic() + timeout
        while True:
            self.pump_one(0.1)
            received = self.diagnostic_markers.get((uri, marker))
            if (received is not None and received >= sent_at
                    and self.compile_status == "compileSuccess"):
                return received
            if time.monotonic() >= deadline:
                raise TimeoutError(
                    f"no successful compilation diagnostic for {marker}: status={self.compile_status}"
                )

    def stabilize_warm(self, quiet_seconds, timeout):
        """Allow startup dependency watches to settle outside measured rounds.

        This is an observed quiet window, not a server-side readiness barrier.
        Success repeats from word-count updates do not restart the window, but
        compilation starts, status transitions and diagnostic publications do.
        """
        started = time.monotonic()
        deadline = started + timeout
        self.event("warm_stabilization_start", quiet_seconds=quiet_seconds)
        while True:
            self.pump_one(min(0.1, max(0, deadline - time.monotonic())))
            now = time.monotonic()
            quiet_since = max(started, self.compile_activity_at)
            if (self.compile_status == "compileSuccess"
                    and now - quiet_since >= quiet_seconds
                    and self.client.messages.empty()):
                elapsed = now - started
                self.event("warm_stabilization_complete", quiet_seconds=quiet_seconds,
                           seconds=elapsed)
                return elapsed
            if now >= deadline:
                raise TimeoutError(
                    f"warm compilation did not stay successful and quiet for {quiet_seconds}s: "
                    f"status={self.compile_status}"
                )

    def stats(self, label, timeout):
        value = self.request("workspace/executeCommand", {
            "command": "tinymist.getServerInfo", "arguments": [],
        }, f"stats-{label}", timeout)
        self.event("stats", label=label, counts=query_counts(value))

    def close(self):
        self.client.close()
        self.events_path.close()


class CapturedClient(runner.LspClient):
    """Preserve actual receipt times and every response, including duplicates."""
    def __init__(self, binary, root, config, log_path, wire_path, started):
        self.wire = wire_path.open("w")
        self.wire_lock = threading.Lock()
        self.started = started
        try:
            super().__init__(binary, root, config, log_path)
        except BaseException:
            self.wire.close()
            raise

    def capture(self, direction, message, now):
        with self.wire_lock:
            self.wire.write(json.dumps({"t": now - self.started, "direction": direction, "message": message}) + "\n")
            self.wire.flush()

    def send(self, message):
        self.capture("send", {"jsonrpc": "2.0", **message}, time.monotonic())
        super().send(message)

    def read_messages(self):
        try:
            while True:
                headers = {}
                while True:
                    line = self.process.stdout.readline()
                    if not line:
                        raise EOFError("server closed stdout")
                    if line in (b"\r\n", b"\n"):
                        break
                    key, value = line.decode("ascii").split(":", 1)
                    headers[key.lower()] = value.strip()
                size = int(headers["content-length"])
                if not 0 < size <= 64 * 1024 * 1024:
                    raise ValueError(f"invalid LSP message size: {size}")
                message = json.loads(runner.read_exact(self.process.stdout, size))
                now = time.monotonic()
                self.capture("receive", message, now)
                self.messages.put((now, message))
        except Exception as error:
            self.messages.put((time.monotonic(), error))

    def close(self):
        super().close()
        self.wire.close()


def trial(args, mode):
    output = args.output_dir / mode
    output.mkdir(parents=True, exist_ok=False)
    (output / "responses").mkdir()
    entry, chapter, text, cursor, heading = fixture(args.book)
    config = {"rootPath": str(args.book), "typstExtraArgs": ["main.typ"],
              "semanticTokens": "enable", "syntaxOnly": "disable", "exportPdf": "never",
              "colorTheme": "dark", "preview": {"refresh": "onType"}, "compileStatus": "enable",
              "lint": {"enabled": False}}
    if args.package_cache:
        config["typstExtraArgs"] += ["--package-cache-path", str(args.package_cache)]
    config["typstExtraArgs"] += ["--font-path", str(args.book / "fonts")]
    result = {"mode": mode, "binary": str(args.binary), "binary_sha256": sha256(args.binary),
              "book": str(args.book), "rounds_requested": args.rounds, "rounds_completed": 0,
              "max_footprint_gib": args.max_footprint_gib, "config": config,
              "warm_quiet_seconds": args.warm_quiet_seconds,
              "mach_timebase": {"numer": TIMEBASE.numer, "denom": TIMEBASE.denom},
              "source_hashes": {str(p): sha256(p) for p in (entry, chapter)},
              "events": [], "memory": [], "responses": {}, "unexpected_responses": [], "diagnostics": {},
              "edit_result_seconds": [],
              "edit_result_metric": "latest edit sent to receipt of its unique compiler-warning diagnostic",
              "edit_markers": [],
              "notes": ["In-memory document edits only; no preview transport measured.",
                        "Process footprint differs from resident memory.",
                        "Completion probes the existing local-tag-scope callback; trace/evaluation counts determine its execution path.",
                        "QueryQueue timing measures semantic admission wait, not internal comemo lock wait.",
                        "Other editor/process activity can affect latency and memory pressure.",
                        "Warmup waits for a successful-compilation quiet window after the warm marker to let dependency watches settle; latest-query errors still fail the run.",
                        "Every version adds an intentional unknown-font warning and tiny fallback glyph; the exact warning proves compilation of that version.",
                        "Edit-result latency includes diagnostic conversion and publication; compileStatus is not revision correlated and is not a timing endpoint."]}
    session = Session(str(args.binary), args.book, config, output, result)
    result["pid"] = session.client.process.pid
    stop = threading.Event()

    def watch():
        while not stop.wait(0.25):
            sample = {"t": round(time.monotonic() - session.started, 6), **memory(result["pid"])}
            result["memory"].append(sample)
            alive = session.client.process.poll() is None
            if alive and (not sample.get("footprint") or sample["footprint"] > args.max_footprint_gib * 1024 ** 3):
                result["limit"] = ("process footprint exceeded diagnostic limit; harness terminated server"
                                   if sample.get("footprint") else "memory guard unavailable; harness terminated server")
                if session.client.process.poll() is None:
                    session.client.process.terminate()
                    try:
                        session.client.process.wait(timeout=2)
                    except subprocess.TimeoutExpired:
                        session.client.process.kill()
                        session.client.process.wait()
                return

    monitor = threading.Thread(target=watch, daemon=True)
    monitor.start()
    try:
        initialized = session.request("initialize", {
            "processId": None, "rootUri": args.book.as_uri(),
            "capabilities": {"workspace": {"configuration": True}, "textDocument": {"semanticTokens": {
                "dynamicRegistration": True, "requests": {"full": {"delta": True}},
                "tokenTypes": [], "tokenModifiers": [], "formats": ["relative"],
            }}}, "initializationOptions": config,
            "workspaceFolders": [{"uri": args.book.as_uri(), "name": "book"}],
        }, "initialize", args.timeout)
        result["server_info"] = initialized.get("serverInfo")
        provider = initialized.get("capabilities", {}).get("semanticTokensProvider")
        if isinstance(provider, dict):
            session.client.legend = provider.get("legend")
        session.client.notify("initialized", {})
        warm_marker = PROBE_PREFIX + "warm"
        warm_sent = time.monotonic()
        for path, content in ((entry, entry.read_text()), (chapter, text.replace(PROBE_SLOT, warm_marker))):
            session.client.notify("textDocument/didOpen", {"textDocument": {
                "uri": path.as_uri(), "languageId": "typst", "version": 1, "text": content,
            }})
        session.request("workspace/executeCommand", {"command": "tinymist.pinMain", "arguments": [str(entry)]},
                        "pin", args.timeout)
        session.settle(chapter.as_uri(), warm_marker, warm_sent, args.timeout)
        session.event("warm_compile")
        result["warm_stabilization_seconds"] = session.stabilize_warm(args.warm_quiet_seconds, args.timeout)
        session.stats("warm", args.timeout)
        td = {"uri": chapter.as_uri()}
        completion = {"textDocument": td, "position": cursor,
                      "context": {"triggerKind": 2, "triggerCharacter": "."}}
        highlight = {"textDocument": td, "position": {**cursor, "character": cursor["character"] - 1}}
        version = 1

        def edit(round_number, suffix=""):
            nonlocal version
            version += 1
            marker = PROBE_PREFIX + f"version-{version:04}"
            body = text.replace(heading, heading + "." * round_number + suffix, 1).replace(PROBE_SLOT, marker)
            session.latest_edit_marker = marker
            session.latest_edit_sent = time.monotonic()
            session.client.notify("textDocument/didChange", {"textDocument": {**td, "version": version},
                                  "contentChanges": [{"text": body}]})
            result["edit_markers"].append({"round": round_number, "version": version, "marker": marker,
                                           "sent_at": session.latest_edit_sent - session.started})
            session.event("edit", round=round_number, version=version, suffix=suffix, marker=marker)

        for number in range(1, args.rounds + 1):
            edit(number)
            ids = []
            prefix = f"round-{number:02}"
            if mode == "semantic":
                ids.append(session.send("textDocument/semanticTokens/full", {"textDocument": td}, prefix + "-semantic"))
            elif mode == "completion":
                ids.append(session.send("textDocument/completion", completion, prefix + "-completion"))
            elif mode in ("mixed", "cancel"):
                active = session.send("textDocument/completion", completion, prefix + "-cancel-active")
                session.pump(args.cancel_delay)
                cancelled = session.send("textDocument/completion", completion, prefix + "-cancel-queued")
                obsolete = session.send("textDocument/semanticTokens/full", {"textDocument": td}, prefix + "-obsolete")
                for request_id in (cancelled, active):
                    session.client.notify("$/cancelRequest", {"id": request_id})
                    session.event("cancel_sent", round=number, request_id=request_id)
                edit(number, " current")
                # Only the latest revision must complete successfully. The
                # baseline can legally ignore cancellation; record its extra
                # responses instead of treating them as a harness failure.
                current = session.send("textDocument/semanticTokens/full", {"textDocument": td}, prefix + "-semantic")
                ids.extend([active, cancelled, obsolete, current])
                if mode == "mixed":
                    ids.extend([
                        session.send("textDocument/completion", completion, prefix + "-completion"),
                        session.send("textDocument/documentHighlight", highlight, prefix + "-highlight"),
                    ])
            if ids:
                responses = session.collect(ids, args.timeout)
                for response in responses:
                    key = response["key"]
                    if "-cancel-" not in key and not key.endswith("-obsolete") and "error" in response["message"]:
                        raise RuntimeError(f"latest request failed: {key}: {response['message']['error']}")
            received = session.settle(chapter.as_uri(), session.latest_edit_marker,
                                      session.latest_edit_sent, args.timeout)
            result["edit_result_seconds"].append(received - session.latest_edit_sent)
            session.stats(prefix, args.timeout)
            result["rounds_completed"] = number
            session.event("round_complete", round=number)
        session.event("idle_start")
        session.pump(args.idle_seconds)
        session.event("idle_end")
        result["completed"] = True
    except Exception as error:
        result["error"] = repr(error)
        session.event("error", error=repr(error))
    finally:
        stop.set()
        monitor.join(3)
        result["legend"] = session.client.legend
        result["pending_requests_at_end"] = {str(request_id): {"key": pending["key"], "method": pending["method"]}
                                             for request_id, pending in session.pending.items()}
        result["duration"] = time.monotonic() - session.started
        session.close()
        result["cache_and_compile_seconds"] = split_logs(output / "server.log", output)
        footprints = [sample["footprint"] for sample in result["memory"] if sample.get("footprint")]
        result["peak_footprint_bytes"] = max(footprints, default=0)
        result["end_footprint_bytes"] = footprints[-1] if footprints else 0
        result["latency_seconds"] = {}
        for method in sorted({response["method"] for key, response in result["responses"].items() if key.startswith("round-")}):
            values = [response["seconds"] for key, response in result["responses"].items()
                      if key.startswith("round-") and response["method"] == method and not response["error"]]
            if values:
                result["latency_seconds"][method] = {"count": len(values), "median": statistics.median(values),
                                                      "p95": sorted(values)[math.ceil(.95 * len(values)) - 1], "max": max(values)}
        result["cancellation_outcomes"] = {
            "cancelled": sum(response["error"] is not None and response["error"].get("code") == -32800
                             for key, response in result["responses"].items() if "-cancel-" in key),
            "content_modified": sum(response["error"] is not None and response["error"].get("code") == -32801
                                    for key, response in result["responses"].items() if key.endswith("-obsolete")),
            "obsolete_succeeded": sum(response["error"] is None for key, response in result["responses"].items()
                                      if key.endswith("-obsolete")),
        }
        if args.require_cancellation and mode in ("mixed", "cancel") and (
            not result["cancellation_outcomes"]["cancelled"] or not result["cancellation_outcomes"]["content_modified"]
        ):
            result["completed"] = False
            result["cancellation_check_error"] = "burst did not demonstrate both cancellation and stale-query rejection"
        if result["unexpected_responses"]:
            result["completed"] = False
            result["duplicate_response_error"] = "unexpected or duplicate terminal responses observed"
        (output / "summary.json").write_text(json.dumps(result, indent=2))
    return result


def latest_latencies(summary, completed_rounds=None):
    """Exclude intentionally cancelled/stale burst work from latest-query latency."""
    values = {}
    for key, response in summary["responses"].items():
        match = re.fullmatch(r"round-(\d+)-(semantic|completion|highlight)", key)
        if match is None or response["error"]:
            continue
        if completed_rounds is not None and int(match[1]) > completed_rounds:
            continue
        values.setdefault(response["method"], []).append(response["seconds"])
    return {
        method: {"count": len(samples), "median": statistics.median(samples),
                 "p95": sorted(samples)[math.ceil(.95 * len(samples)) - 1], "max": max(samples)}
        for method, samples in values.items()
    }


def compare(left, right, output):
    comparisons = {}
    for left_path in sorted(left.glob("*/summary.json")):
        right_path = right / left_path.parent.name / "summary.json"
        if not right_path.exists():
            continue
        a, b = json.loads(left_path.read_text()), json.loads(right_path.read_text())
        keys = sorted(set(a["responses"]) & set(b["responses"]))
        matched, mismatched, unavailable = [], [], []
        for key in keys:
            if not key.startswith("round-"):
                continue
            one, two = a["responses"][key], b["responses"][key]
            if one["error"] or two["error"]:
                unavailable.append({"key": key, "left_error": one["error"], "right_error": two["error"]})
            elif one["canonical_sha256"] == two["canonical_sha256"]:
                matched.append(key)
            else:
                mismatched.append(key)
        shared_rounds = min(a["rounds_completed"], b["rounds_completed"])
        comparisons[left_path.parent.name] = {
            "left_rounds": a["rounds_completed"], "right_rounds": b["rounds_completed"],
            "legend_matches": a["legend"] == b["legend"],
            "matched_responses": matched, "mismatched_responses": mismatched,
            "unavailable_comparisons": unavailable,
            "left_peak_footprint": a["peak_footprint_bytes"], "right_peak_footprint": b["peak_footprint_bytes"],
            "left_latency": a["latency_seconds"], "right_latency": b["latency_seconds"],
            "left_edit_result_seconds": a.get("edit_result_seconds", []),
            "right_edit_result_seconds": b.get("edit_result_seconds", []),
            "edit_result_metric_matches": bool(a.get("edit_result_metric")) and a.get("edit_result_metric") == b.get("edit_result_metric"),
            "warmup_policy_matches": (a.get("warm_quiet_seconds") is not None
                                      and a.get("warm_quiet_seconds") == b.get("warm_quiet_seconds")),
            "left_latest_latency": latest_latencies(a, a["rounds_completed"]),
            "right_latest_latency": latest_latencies(b, b["rounds_completed"]),
            "comparable_completed_rounds": shared_rounds,
            "left_common_round_latency": latest_latencies(a, shared_rounds),
            "right_common_round_latency": latest_latencies(b, shared_rounds),
            "left_completed": a.get("completed", False), "right_completed": b.get("completed", False),
            "left_limit": a.get("limit"), "right_limit": b.get("limit"),
            "left_cancellation": a.get("cancellation_outcomes"), "right_cancellation": b.get("cancellation_outcomes"),
        }
    left_inputs, right_inputs = left / "inputs-before.json", right / "inputs-before.json"
    if left_inputs.exists() and right_inputs.exists():
        comparisons["input_sources_match"] = (json.loads(left_inputs.read_text())["sha256"]
                                                == json.loads(right_inputs.read_text())["sha256"])
    output.write_text(json.dumps(comparisons, indent=2))
    print(json.dumps(comparisons, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", "--output-dir", dest="output_dir", type=Path)
    parser.add_argument("--book", type=Path, help="Isolated PDE book snapshot")
    parser.add_argument("--modes", "--mode", dest="mode", choices=["compile", "semantic", "completion", "mixed", "cancel"], nargs="+", default=["compile", "semantic", "completion", "mixed"])
    parser.add_argument("--rounds", type=int, default=12)
    parser.add_argument("--timeout", type=float, default=90)
    parser.add_argument("--max-footprint-gib", type=float, default=5)
    parser.add_argument("--cancel-delay", type=float, default=0.05)
    parser.add_argument("--idle-seconds", type=float, default=5)
    parser.add_argument("--warm-quiet-seconds", type=float, default=3,
                        help="successful-compilation quiet window before measured edits (default: 3)")
    parser.add_argument("--package-cache", type=Path, default=Path.home() / "Library/Caches/typst/packages")
    parser.add_argument("--require-cancellation", action="store_true", help="require mixed/cancel modes to demonstrate cancelled and stale responses (candidate only)")
    parser.add_argument("--compare", type=Path, nargs=2, metavar=("BASELINE_DIR", "CANDIDATE_DIR"))
    args = parser.parse_args()
    if args.compare:
        output = args.output_dir or BASE
        output.mkdir(parents=True, exist_ok=True)
        compare(*args.compare, output / "comparison.json")
        return
    if not args.binary or not args.output_dir or not args.book:
        parser.error("--binary, --output-dir and --book are required")
    if args.rounds < 1 or not 0 < args.max_footprint_gib <= 5:
        parser.error("rounds must be positive and the memory limit must be at most 5 GiB")
    if not 0 < args.warm_quiet_seconds < args.timeout:
        parser.error("warm quiet seconds must be positive and shorter than the request timeout")
    args.binary, args.book = Path(runner.resolve_binary(str(args.binary))), args.book.resolve()
    args.package_cache = args.package_cache.resolve()
    if LIBPROC is None:
        parser.error("this harness requires macOS process-footprint monitoring to enforce its memory guard")
    if not args.package_cache.is_dir():
        parser.error("the original package cache must exist")
    args.output_dir = args.output_dir.resolve()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    roots = {"book": args.book, "package-cache": args.package_cache,
             "local-packages": Path.home() / "Library/Application Support/typst/packages"}
    before = source_manifest(roots)
    (args.output_dir / "inputs-before.json").write_text(json.dumps(before, indent=2))
    os.environ["TINYMIST_LOG"] = "info,tinymist_project::compiler=debug,tinymist::query_queue=debug"
    results = [trial(args, mode) for mode in args.mode]
    after = source_manifest(roots)
    (args.output_dir / "inputs-after.json").write_text(json.dumps(after, indent=2))
    integrity = {"unchanged": before == after, "before_sha256": before["sha256"], "after_sha256": after["sha256"],
                 "checked_extensions": [".typ", ".toml", ".bib"], "file_count": before["file_count"]}
    (args.output_dir / "input-integrity.json").write_text(json.dumps(integrity, indent=2))
    (args.output_dir / "summary.json").write_text(json.dumps(results, indent=2))
    if not integrity["unchanged"] or any(not result.get("completed") for result in results):
        sys.exit(1)


if __name__ == "__main__":
    main()
