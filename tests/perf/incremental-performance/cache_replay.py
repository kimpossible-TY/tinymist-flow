#!/usr/bin/env python3
"""Measure a real Maquette redraw after outer caches hide its byte results.

All document changes go through LSP memory overlays. A hidden 0.1pt font probe
inside the figure confirms that the exact figure revision reached layout. The
probe is not sent to Maquette; nx=64 versus nx=32*2 preserves integer values,
mesh bytes and render configuration. This harness does not export or compare
PDFs, so it reports page counts and cache evidence without claiming complete
output equivalence. Run only after builds have stopped, one engine at a time.
"""
import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import threading
import time

spec = importlib.util.spec_from_file_location(
    "flow_book_replay", Path(__file__).with_name("book_replay.py")
)
replay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(replay)

FIGURES_ANCHOR = "#let coordinate-averaging-surfaces() = context {\n"
FIGURES_SLOT = "__FLOW_CACHE_FIGURE_FONT__"


def sweeps(path):
    records = []
    if not path.exists():
        return records
    for index, line in enumerate(path.read_text(errors="replace").splitlines()):
        if "evict comemo cache in " not in line:
            continue
        record = {"line": line, "log_line_index": index}
        for key in ("protected_entries", "protected_payload_bytes", "protected_hits",
                    "expensive_compute_count", "payload_budget"):
            match = re.search(rf"\b{key}=(\d+)", line)
            if match:
                record[key] = int(match[1])
        match = re.search(r"expensive_compute_time=([^,)]+)", line)
        if match:
            record["expensive_compute_seconds"] = replay.seconds(match[1])
        records.append(record)
    return records


def phase_events(lines, entry):
    """Preserve log order; wall-clock timestamps only have second precision."""
    events = []
    for index, line in enumerate(lines):
        if "compilation succeeded" in line:
            kind = "compiled" if f"{entry}:" in line else "ambiguous"
        elif "automatic cache sweep queued for " in line:
            # This is emitted when a new sweep worker starts, before its sweep.
            kind = "started"
        elif "evict comemo cache in " in line:
            kind = "swept"
        elif "outcome=discarded" in line or "compilation failed" in line:
            kind = "ambiguous"
        else:
            continue
        events.append((index, kind))
    return events


def validate_phase_events(events, *, warm=False):
    """Require a fresh sweep whose counter read follows the completed compile.

    A previously running/coalesced sweep is deliberately insufficient. Without
    revision IDs on sweeps, concurrent or extra work cannot be disambiguated.
    Warm-up may have earlier complete activity, but its final chain must match.
    """
    compiled = [i for i, (_, kind) in enumerate(events) if kind == "compiled"]
    if not compiled:
        return None
    if not warm and len(compiled) != 1:
        raise ValueError("expected exactly one successful main compile in the measured phase")
    chain = events[compiled[-1]:] if warm else events
    kinds = [kind for _, kind in chain]
    if kinds in (["compiled"], ["compiled", "started"]):
        return None  # The fresh worker can take longer than the quiet window.
    if kinds != ["compiled", "started", "swept"]:
        raise ValueError(f"ambiguous compile/sweep phase: {chain}")
    return chain[-1][0]


def settle_sweep(session, entry, log_checkpoint, timeout, quiet_seconds, *, warm=False):
    """Select a post-compilation counter snapshot, then require observed quiet.

    The quiet window is additional protection against dependency-watch work;
    the ordered fresh-worker chain provides the counter sampling boundary.
    This remains an observed protocol/log boundary, not an engine idle API.
    """
    deadline = time.monotonic() + timeout
    log = session.output / "server.log"
    quiet_since = time.monotonic()
    previous = None
    while True:
        session.pump(0.1)
        now = time.monotonic()
        lines = log.read_text(errors="replace").splitlines()
        events = phase_events(lines[log_checkpoint:], entry)
        signature = (events, session.compile_activity_at)
        if signature != previous:
            previous = signature
            quiet_since = now
        quiet = (session.compile_status == "compileSuccess"
                 and session.client.messages.empty()
                 and now - quiet_since >= quiet_seconds)
        if quiet and events:
            # Fail closed once an ambiguous phase has settled; do not pick a
            # convenient earlier counter record or silently ignore extra work.
            selected = validate_phase_events(events, warm=warm)
            if selected is not None:
                selected += log_checkpoint
                records = sweeps(log)
                if not records or records[-1]["log_line_index"] != selected:
                    # The log changed between reads. Observe the new complete state.
                    previous = None
                else:
                    session.event("cache_phase_settled", warm=warm, quiet_seconds=quiet_seconds,
                                  log_checkpoint=log_checkpoint, selected_log_line=selected,
                                  completed_sweeps=len(records))
                    return records
        if now >= deadline:
            raise TimeoutError("no unambiguous completed compile/sweep chain with quiet counters")


def figures_fixture(path):
    source = path.read_text()
    if source.count(FIGURES_ANCHOR) != 1:
        raise ValueError("expected one coordinate-averaging-surfaces definition")
    probe = f'  place(top + left, hide(text(font: "{FIGURES_SLOT}", size: 0.1pt)[.]))\n'
    source = source.replace(FIGURES_ANCHOR, FIGURES_ANCHOR + probe, 1)
    start = source.index(FIGURES_ANCHOR)
    end = source.index("#let coordinate-averaging-diagram()", start)
    if source[start:end].count("let nx = 64") != 1:
        raise ValueError("expected the unchanged Maquette mesh resolution")
    changed = source[:start] + source[start:end].replace("let nx = 64", "let nx = 32 * 2", 1) + source[end:]
    return source, changed


def run(binary, args, name):
    output = args.output / name
    output.mkdir(parents=True, exist_ok=False)
    (output / "responses").mkdir()
    entry, chapter, chapter_text, _, _ = replay.fixture(args.book)
    figures = args.book / "chapter 3/figures/figures.typ"
    figures_original, figures_changed = figures_fixture(figures)
    config = {
        "rootPath": str(args.book),
        "typstExtraArgs": ["main.typ", "--font-path", str(args.book / "fonts"),
                           "--package-cache-path", str(args.package_cache)],
        "semanticTokens": "enable", "syntaxOnly": "disable", "exportPdf": "never",
        "colorTheme": "dark", "compileStatus": "enable", "lint": {"enabled": False},
    }
    result = {
        "mode": "cache-retention", "engine": name, "binary": str(binary),
        "binary_sha256": replay.sha256(binary), "config": config,
        "settle_quiet_seconds": args.settle_quiet_seconds, "timeout_seconds": args.timeout,
        "events": [], "responses": {}, "unexpected_responses": [], "diagnostics": {},
        "memory": [], "stages": [], "notes": [
            "Only LSP memory overlays are modified; no book/package file is edited.",
            "The figures function contains a hidden placed 0.1pt missing-font probe; warning delivery proves its latest revision reached layout.",
            "Probe content never enters Maquette arguments; nx=32*2 is the same integer 64 as nx=64.",
            "Each phase requires a successful main compile followed in log order by a fresh sweep worker and completed sweep, then observed LSP/log quiet.",
            "At least two completed automatic eviction sweeps are observed during unrelated chapter probe edits.",
            "Protected hits count lookup events, not distinct outputs; costly computation counts exclude calls below the configured threshold.",
            "Counts cover eligible costly byte computations, not all cache misses or identified plugin functions.",
            "No PDF/bitmap comparison is performed; exact complete-document output equality is not established by this harness.",
        ],
    }
    session = replay.Session(str(binary), args.book, config, output, result)
    result["pid"] = session.client.process.pid
    stop = threading.Event()

    def monitor_memory():
        while not stop.wait(0.25):
            sample = {"t": time.monotonic() - session.started, **replay.memory(result["pid"])}
            result["memory"].append(sample)
            process = session.client.process
            if process.poll() is None and (not sample.get("footprint") or
                    sample["footprint"] > args.max_footprint_gib * 1024 ** 3):
                result["limit"] = "footprint guard unavailable or exceeded; terminated this harness's server"
                process.terminate()
                try:
                    process.wait(timeout=2)
                except replay.subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                return

    monitor = threading.Thread(target=monitor_memory, daemon=True)
    monitor.start()
    try:
        initialized = session.request("initialize", {
            "processId": None, "rootUri": args.book.as_uri(),
            "capabilities": {"workspace": {"configuration": True}},
            "initializationOptions": config,
            "workspaceFolders": [{"uri": args.book.as_uri(), "name": "book"}],
        }, "initialize", args.timeout)
        result["server_info"] = initialized.get("serverInfo")
        log = output / "server.log"
        checkpoint = len(log.read_text(errors="replace").splitlines())
        session.client.notify("initialized", {})
        sent = time.monotonic()
        for path, text in (
            (figures, figures_original.replace(FIGURES_SLOT, "flow-replay-confirm-figure-warm")),
            (chapter, chapter_text.replace(replay.PROBE_SLOT, "flow-replay-confirm-chapter-warm")),
            (entry, entry.read_text()),
        ):
            session.client.notify("textDocument/didOpen", {"textDocument": {
                "uri": path.as_uri(), "languageId": "typst", "version": 1, "text": text,
            }})
        session.request("workspace/executeCommand", {
            "command": "tinymist.pinMain", "arguments": [str(entry)],
        }, "pin", args.timeout)
        session.settle(figures.as_uri(), "flow-replay-confirm-figure-warm", sent, args.timeout)
        session.settle(chapter.as_uri(), "flow-replay-confirm-chapter-warm", sent, args.timeout)
        session.stabilize_warm(args.settle_quiet_seconds, args.timeout)
        records = settle_sweep(session, entry, checkpoint, args.timeout,
                               args.settle_quiet_seconds, warm=True)
        result["warm_sweeps"] = records
        session.event("warm_complete", completed_sweeps=len(records))

        # Figure source stays byte-identical during these edits: its outer
        # memoized work can hide the inner plugin result from direct cache hits.
        for round_number in range(1, args.age_rounds + 1):
            checkpoint = len(log.read_text(errors="replace").splitlines())
            marker = f"flow-replay-confirm-age-{round_number}"
            body = chapter_text.replace(replay.PROBE_SLOT, marker)
            sent = time.monotonic()
            session.client.notify("textDocument/didChange", {
                "textDocument": {"uri": chapter.as_uri(), "version": round_number + 1},
                "contentChanges": [{"text": body}],
            })
            received = session.settle(chapter.as_uri(), marker, sent, args.timeout)
            records = settle_sweep(session, entry, checkpoint, args.timeout,
                                   args.settle_quiet_seconds)
            stage = {"stage": f"age-{round_number}", "edit_result_seconds": received - sent,
                     "source_sha256": replay.hashlib.sha256(body.encode()).hexdigest(),
                     "completed_sweeps": len(records), "last_sweep": records[-1]}
            result["stages"].append(stage)
            session.event("age_complete", **stage)

        result["before_trigger_sweeps"] = records
        checkpoint = len(log.read_text(errors="replace").splitlines())
        marker = "flow-replay-confirm-figure-trigger"
        body = figures_changed.replace(FIGURES_SLOT, marker)
        sent = time.monotonic()
        session.client.notify("textDocument/didChange", {
            "textDocument": {"uri": figures.as_uri(), "version": 2},
            "contentChanges": [{"text": body}],
        })
        received = session.settle(figures.as_uri(), marker, sent, args.timeout)
        records = settle_sweep(session, entry, checkpoint, args.timeout,
                               args.settle_quiet_seconds)
        result["trigger"] = {"edit_result_seconds": received - sent,
                             "source_sha256": replay.hashlib.sha256(body.encode()).hexdigest(),
                             "last_sweep": records[-1]}
        result["after_trigger_sweeps"] = records
        session.stats("trigger", args.timeout)
        session.pump(1)
        result["completed"] = True
    except Exception as error:
        result["error"] = repr(error)
        session.event("error", error=repr(error))
    finally:
        stop.set()
        monitor.join(3)
        result["duration"] = time.monotonic() - session.started
        result["sweeps"] = sweeps(output / "server.log")
        result["compile_successes"] = session.compile_successes
        session.close()
        result["cache_and_compile_seconds"] = replay.split_logs(output / "server.log", output)
        result["peak_footprint_bytes"] = max((s.get("footprint", 0) for s in result["memory"]), default=0)
        (output / "summary.json").write_text(json.dumps(result, indent=2))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--book", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--package-cache", type=Path, default=Path.home() / "Library/Caches/typst/packages")
    parser.add_argument("--timeout", type=float, default=300)
    parser.add_argument("--age-rounds", type=int, default=2)
    parser.add_argument("--settle-quiet-seconds", type=float, default=3,
                        help="successful LSP/log quiet required after every compile/sweep phase")
    parser.add_argument("--max-footprint-gib", type=float, default=4.5)
    parser.add_argument("--require-retention", action="store_true",
                        help="fail unless protected outputs survive aging and are reused without costly recomputation")
    args = parser.parse_args()
    if (args.age_rounds < 2 or not 0 < args.max_footprint_gib <= 5
            or not math.isfinite(args.timeout) or not math.isfinite(args.settle_quiet_seconds)
            or not 0.5 <= args.settle_quiet_seconds < args.timeout):
        parser.error("require at least two aging rounds, finite timeout > quiet >= 0.5 s, and a footprint guard at most 5 GiB")
    if replay.LIBPROC is None:
        parser.error("requires macOS process-footprint monitoring")
    for name in ("baseline", "candidate", "book", "output", "package_cache"):
        setattr(args, name, getattr(args, name).resolve())
    args.output.mkdir(parents=True, exist_ok=False)
    roots = {"book": args.book, "package-cache": args.package_cache,
             "local-packages": Path.home() / "Library/Application Support/typst/packages"}
    before = replay.source_manifest(roots)
    plugin = args.package_cache / "preview/maquette/0.1.3"
    plugin_before = {name: replay.sha256(plugin / name) for name in ("maquette.typ", "maquette.wasm")}
    (args.output / "inputs-before.json").write_text(json.dumps({"sources": before, "maquette": plugin_before}, indent=2))
    os.environ["TINYMIST_LOG"] = "info,tinymist_project::compiler=debug"
    baseline = run(args.baseline, args, "baseline")
    candidate = run(args.candidate, args, "candidate")
    after = replay.source_manifest(roots)
    plugin_after = {name: replay.sha256(plugin / name) for name in plugin_before}
    comparison = {"inputs_unchanged": before == after and plugin_before == plugin_after,
                  "baseline_completed": baseline.get("completed", False),
                  "candidate_completed": candidate.get("completed", False),
                  "baseline_trigger_seconds": baseline.get("trigger", {}).get("edit_result_seconds"),
                  "candidate_trigger_seconds": candidate.get("trigger", {}).get("edit_result_seconds"),
                  "candidate_cache_evidence": None,
                  "settle_quiet_seconds": args.settle_quiet_seconds,
                  "output_equivalence": "not measured; mesh/config preserved by integer identity; full document output comparison remains separate"}
    if candidate.get("completed"):
        warm = candidate["warm_sweeps"][-1]
        aged = candidate["before_trigger_sweeps"][-1]
        triggered = candidate["trigger"]["last_sweep"]
        required = ("protected_hits", "expensive_compute_count", "protected_entries", "protected_payload_bytes")
        if all(key in record for record in (warm, aged, triggered) for key in required):
            evidence = {
                "aging_protected_hits": aged["protected_hits"] - warm["protected_hits"],
                "aging_expensive_computations": aged["expensive_compute_count"] - warm["expensive_compute_count"],
                "trigger_protected_hits": triggered["protected_hits"] - aged["protected_hits"],
                "trigger_expensive_computations": triggered["expensive_compute_count"] - aged["expensive_compute_count"],
                "protected_working_set_stable": (
                    warm["protected_entries"] == aged["protected_entries"] == triggered["protected_entries"] >= 4
                    and warm["protected_payload_bytes"] == aged["protected_payload_bytes"] == triggered["protected_payload_bytes"] > 0),
                "interpretation": "Aggregate protected lookup events, not distinct Maquette outputs; zero costly eligible computations does not exclude cheaper calls.",
            }
            evidence["supports_hidden_result_retention"] = (
                evidence["aging_protected_hits"] == 0
                and evidence["aging_expensive_computations"] == 0
                and evidence["trigger_protected_hits"] >= 4
                and evidence["trigger_expensive_computations"] == 0
                and evidence["protected_working_set_stable"])
            comparison["candidate_cache_evidence"] = evidence
    (args.output / "comparison.json").write_text(json.dumps(comparison, indent=2))
    print(json.dumps(comparison, indent=2))
    if not comparison["inputs_unchanged"] or not baseline.get("completed") or not candidate.get("completed"):
        raise SystemExit(1)
    if args.require_retention and not (comparison["candidate_cache_evidence"] or {}).get("supports_hidden_result_retention"):
        raise SystemExit("candidate did not demonstrate hidden-result retention")


if __name__ == "__main__":
    main()
