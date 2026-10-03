#!/usr/bin/env python3
"""Compare themed preview work using disposable sources, ports, and focus records.

Uses only the Python standard library. Each executable runs alone. Timings are
small-fixture observations, not predictions for a user's document. A preview's
latest content is checked through hit testing against its rendered revision.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import struct
import subprocess
import tempfile
import time


class WebSocket:
    def __init__(self, port: int, route: str):
        self.socket = socket.create_connection(("127.0.0.1", port), timeout=5)
        self.buffer = bytearray()
        self.fragments = bytearray()
        self.fragment_opcode = None
        key = base64.b64encode(os.urandom(16)).decode()
        request = (
            f"GET {route} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n"
            f"Origin: http://127.0.0.1:{port}\r\nUpgrade: websocket\r\n"
            f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
            "Sec-WebSocket-Version: 13\r\n\r\n"
        )
        self.socket.sendall(request.encode())
        while b"\r\n\r\n" not in self.buffer:
            chunk = self.socket.recv(4096)
            if not chunk:
                raise RuntimeError("WebSocket upgrade ended early")
            self.buffer.extend(chunk)
        header, _, rest = self.buffer.partition(b"\r\n\r\n")
        self.buffer = bytearray(rest)
        expected = base64.b64encode(hashlib.sha1(
            (key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()
        ).digest())
        if b" 101 " not in header.split(b"\r\n", 1)[0] or expected not in header:
            raise RuntimeError(f"WebSocket upgrade rejected: {header!r}")

    def send(self, value: str | bytes, opcode: int = 1):
        payload = value.encode() if isinstance(value, str) else value
        mask = os.urandom(4)
        size = len(payload)
        if size < 126:
            header = bytes((0x80 | opcode, 0x80 | size))
        elif size < 65536:
            header = bytes((0x80 | opcode, 0x80 | 126)) + struct.pack("!H", size)
        else:
            header = bytes((0x80 | opcode, 0x80 | 127)) + struct.pack("!Q", size)
        self.socket.sendall(header + mask + bytes(
            value ^ mask[index % 4] for index, value in enumerate(payload)
        ))

    def receive(self, timeout: float):
        deadline = time.monotonic() + timeout
        while True:
            if len(self.buffer) >= 2:
                first, second = self.buffer[:2]
                size, offset = second & 127, 2
                extended = 2 if size == 126 else 8 if size == 127 else 0
                if len(self.buffer) >= 2 + extended:
                    if extended:
                        size = int.from_bytes(self.buffer[2:2 + extended], "big")
                        offset += extended
                    if second & 128:
                        raise RuntimeError("Server sent a masked WebSocket frame")
                    if size > 64 * 1024 * 1024:
                        raise RuntimeError("Unexpectedly large fixture preview frame")
                    if len(self.buffer) >= offset + size:
                        payload = bytes(self.buffer[offset:offset + size])
                        del self.buffer[:offset + size]
                        opcode = first & 15
                        if opcode == 8:
                            raise EOFError("Preview WebSocket closed")
                        if opcode == 9:
                            self.send(payload, opcode=10)
                            continue
                        if opcode == 10:
                            continue
                        if opcode != 0:
                            self.fragment_opcode = opcode
                        self.fragments.extend(payload)
                        if first & 128:
                            message = (self.fragment_opcode, bytes(self.fragments))
                            self.fragments.clear()
                            self.fragment_opcode = None
                            return message
                        continue
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return None
            self.socket.settimeout(remaining)
            try:
                chunk = self.socket.recv(65536)
            except socket.timeout:
                return None
            if not chunk:
                raise EOFError("Preview WebSocket reached EOF")
            self.buffer.extend(chunk)

    def close(self):
        # Exercise EOF cleanup without depending on a close-frame handshake.
        try:
            self.socket.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        self.socket.close()


class Viewer:
    def __init__(self, port: int, theme: str, focus: Path):
        self.ws = WebSocket(port, f"/_theme/{theme}")
        self.focus = focus
        self.revision = None
        self.frames = 0
        self.ws.send("current")

    def wait_for_source(self, marker: str, timeout: float):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            message = self.ws.receive(max(0, deadline - time.monotonic()))
            if message is None:
                break
            _, payload = message
            if payload.startswith(b"focus-revision,"):
                self.revision = payload.split(b",", 1)[1].decode()
            elif payload.startswith((b"diff-v1,", b"new,")):
                self.frames += 1
                if self.revision is not None:
                    self.ws.send("src-point " + json.dumps({
                        "page_no": 1, "x": 20, "y": 20, "revision": self.revision,
                    }))
            elif payload.startswith(b"focus,"):
                response = json.loads(payload.split(b",", 1)[1])
                record = json.loads(self.focus.read_text())
                if (response.get("status") == "selected"
                        and record.get("rendered_revision") == self.revision
                        and record.get("compiled_revision") == self.revision
                        and marker in (record.get("source") or {}).get("excerpt", "")):
                    return self.revision
                if response.get("status") == "unmapped":
                    raise AssertionError(f"Fixture hit test did not map to source: {record}")
        raise TimeoutError(f"Latest preview source not delivered: {marker}, revision={self.revision}")

    def close(self):
        self.ws.close()


def write_document(path: Path, revision: int):
    marker = f"revision-{revision:04d}"
    text = (
        '#set page(width: 240pt, height: 120pt, margin: 10pt)\n'
        '#let palette = sys.inputs.at("theme", default: "light")\n'
        '#set page(fill: if palette == "dark" { black } else { white })\n'
        '#set text(size: 12pt, fill: if palette == "dark" { white } else { black })\n'
        f'= {marker}\n'
        'theme: #palette\n'
    )
    replacement = path.with_suffix(".new")
    replacement.write_text(text)
    replacement.replace(path)
    return marker


def compile_count(log: Path):
    return log.read_text(errors="replace").count("compilation succeeded in")


def quiet(log: Path, timeout: float, window: float):
    deadline = time.monotonic() + timeout
    count, unchanged = compile_count(log), time.monotonic()
    while time.monotonic() < deadline:
        time.sleep(0.05)
        latest = compile_count(log)
        if latest != count:
            count, unchanged = latest, time.monotonic()
        if time.monotonic() - unchanged >= window:
            return count
    raise TimeoutError("Preview compilation did not settle")


def run_engine(engine: Path, label: str, args):
    with tempfile.TemporaryDirectory(prefix="flow-preview-demand-") as directory:
        root = Path(directory).resolve()
        document, focus, log = root / "main.typ", root / "focus.json", root / "engine.log"
        marker = write_document(document, 0)
        env = dict(os.environ, TINYMIST_PREVIEW_FOCUS_FILE=str(focus))
        # Never inherit the real service's reading-position persistence file.
        env.pop("TINYMIST_PREVIEW_CHANGE_FILE", None)
        # A global `info` does not override the CLI's more specific tinymist
        # warning filter. Count compiler events with an explicit target filter.
        log_filter = "tinymist_project::compiler=info,tinymist::compat::preview=info"
        command = [str(engine), "--log-filter", log_filter, "preview", str(document),
                   "--root", str(root), "--follow-system-theme", "--no-open",
                   "--data-plane-host=127.0.0.1:0", "--control-plane-host=127.0.0.1:0"]
        viewers = []
        result = {"label": label, "engine": str(engine), "log_filter": log_filter, "stages": []}
        with log.open("wb") as output:
            process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT, env=env)
            try:
                deadline = time.monotonic() + args.timeout
                port = None
                while time.monotonic() < deadline:
                    match = re.search(r"Data plane server listening on: 127\.0\.0\.1:(\d+)",
                                      log.read_text(errors="replace"))
                    if match:
                        port = int(match.group(1))
                        break
                    if process.poll() is not None:
                        raise RuntimeError(f"Preview exited: {log.read_text(errors='replace')}")
                    time.sleep(0.05)
                if port is None:
                    raise TimeoutError("Preview did not open its data-plane listener")

                def settle_stage(name, before, started, observed=None):
                    delivered = time.monotonic() - started if observed else None
                    count = quiet(log, args.timeout, args.idle_window)
                    result["stages"].append({
                        "name": name, "compiles": count - before,
                        "delivery_seconds": delivered,
                        "revisions": observed or [],
                    })

                before, started = 0, time.monotonic()
                if label == "baseline":
                    # The baseline eagerly compiles both themes, including cold
                    # font discovery. Count them before the first viewer phase.
                    deadline = time.monotonic() + args.timeout
                    while compile_count(log) < 2 and time.monotonic() < deadline:
                        time.sleep(0.05)
                    if compile_count(log) < 2:
                        raise TimeoutError("Baseline did not compile both themes at startup")
                settle_stage("startup_without_viewers", before, started)
                before, started = compile_count(log), time.monotonic()
                light = Viewer(port, "light", focus)
                viewers.append(light)
                observed = [light.wait_for_source(marker, args.timeout)]
                settle_stage("first_light_viewer", before, started, observed)

                def edit(name, revision, active):
                    before, started = compile_count(log), time.monotonic()
                    current = write_document(document, revision)
                    observed = [viewer.wait_for_source(current, args.timeout) for viewer in active]
                    settle_stage(name, before, started, observed)
                    return current

                marker = edit("edit_light_only", 1, [light])
                before, started = compile_count(log), time.monotonic()
                dark = Viewer(port, "dark", focus)
                viewers.append(dark)
                observed = [dark.wait_for_source(marker, args.timeout)]
                settle_stage("connect_dark_viewer", before, started, observed)
                marker = edit("edit_both_themes", 2, [light, dark])

                dark.close()
                viewers.remove(dark)
                time.sleep(0.2)
                marker = edit("edit_after_dark_disconnect", 3, [light])
                light.close()
                viewers.remove(light)
                time.sleep(0.2)
                marker = edit("edit_without_viewers", 4, [])

                before, started = compile_count(log), time.monotonic()
                dark = Viewer(port, "dark", focus)
                viewers.append(dark)
                observed = [dark.wait_for_source(marker, args.timeout)]
                settle_stage("reconnect_dark_latest_source", before, started, observed)
            finally:
                for viewer in viewers:
                    viewer.close()
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
                destination = args.output.with_name(f"{args.output.stem}-{label}.log")
                destination.write_bytes(log.read_bytes())
                result["log"] = str(destination)
        if label == "candidate":
            stages = {stage["name"]: stage for stage in result["stages"]}
            assert stages["startup_without_viewers"]["compiles"] == 0, result
            assert stages["edit_without_viewers"]["compiles"] == 0, result
            for name in ("edit_light_only", "edit_after_dark_disconnect"):
                assert stages[name]["compiles"] == 1, result
            assert stages["edit_both_themes"]["compiles"] == 2, result
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--timeout", default=30.0, type=float)
    parser.add_argument("--idle-window", default=1.5, type=float)
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    results = []
    for label in ("baseline", "candidate"):
        engine = getattr(args, label)
        if engine is not None:
            results.append(run_engine(engine.resolve(), label, args))
            args.output.write_text(json.dumps(results, ensure_ascii=False, indent=2) + "\n")
    print(args.output)


if __name__ == "__main__":
    main()
