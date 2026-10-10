"""Optional external-engine packaging and real preview delivery in temporary paths."""
import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('flow_bundle', ROOT / 'scripts/flow-app.py')
package = importlib.util.module_from_spec(spec); spec.loader.exec_module(package)


def exact(reader, length):
    data = reader.read(length)
    if len(data) != length: raise RuntimeError('Preview WebSocket closed before delivering a frame')
    return data


def document_frame(port, path, origin):
    with socket.create_connection(('127.0.0.1', port), timeout=10) as client:
        key = base64.b64encode(os.urandom(16)).decode()
        request = (f'GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n'
                   f'Upgrade: websocket\r\nConnection: Upgrade\r\n'
                   f'Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nOrigin: {origin}\r\n\r\n')
        client.sendall(request.encode())
        reader = client.makefile('rb')
        if b' 101 ' not in reader.readline(): raise RuntimeError('Preview refused the configured HTTPS origin')
        while True:
            header = reader.readline()
            if header == b'\r\n': break
            if not header: raise RuntimeError('Preview closed during WebSocket handshake')
        payload = b'current'; mask = os.urandom(4)
        client.sendall(bytes([0x81, 0x80 | len(payload)]) + mask + bytes(value ^ mask[i % 4] for i, value in enumerate(payload)))
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            first, second = exact(reader, 2)
            length = second & 0x7f
            if length == 126: length = struct.unpack('!H', exact(reader, 2))[0]
            elif length == 127: length = struct.unpack('!Q', exact(reader, 8))[0]
            if second & 0x80: raise RuntimeError('Unexpected masked server frame')
            data = exact(reader, length)
            if first & 0x0f == 8: raise RuntimeError('Preview WebSocket closed')
            if data.partition(b',')[0] in (b'new', b'diff-v1'): return data
        raise RuntimeError('No compiled document frame received')


@unittest.skipUnless(sys.platform == 'darwin' and os.environ.get('FLOW_TEST_ENGINE'), 'Set FLOW_TEST_ENGINE on macOS for integration checks')
class ExternalEngineBundle(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # The external engine treats /var and /private/var as different project roots.
        temp = tempfile.TemporaryDirectory(prefix='.flow-bundle-tests-', dir=Path.home()); cls.addClassCleanup(temp.cleanup)
        cls.root = Path(temp.name).resolve()
        cls.app = cls.root / 'built/tinymist-flow.app'
        cls.engine = Path(os.environ['FLOW_TEST_ENGINE']).expanduser().resolve()
        cls.input_hash = package.digest(cls.engine)
        cls.env = {'FLOW_DATA_DIR': str(cls.root / 'data'), 'FLOW_LOG_DIR': str(cls.root / 'logs')}
        with patch.dict(os.environ, cls.env):
            package.build(argparse.Namespace(engine=cls.engine, output=cls.app, version='0.1.0', identity=os.environ.get('FLOW_TEST_SIGN_IDENTITY', '-')))

    def test_provenance_signature_and_isolated_install(self):
        with patch.dict(os.environ, self.env):
            manifest = package.validate(self.app)
            self.assertEqual(manifest['engineSource'], 'external')
            self.assertEqual(manifest['engineInputSHA256'], self.input_hash)
            self.assertEqual(manifest['engineVersion'], package.capture([self.engine, '--version']))
            destination = self.root / 'installed/tinymist-flow.app'
            package.replace_app(self.app, destination, self.root / 'install-data')
            self.assertEqual(package.validate(destination)['engineSHA256'], manifest['engineSHA256'])
        self.assertEqual(package.digest(self.engine), self.input_hash)

    def test_preview_delivers_both_themes_through_configured_origin(self):
        project = self.root / 'project'; project.mkdir()
        (project / 'main.typ').write_text('#let dark = sys.inputs.at("theme", default: "light") == "dark"\n'
                                        '#set page(width: 10cm, height: 10cm, fill: if dark { black } else { white })\n'
                                        '#set text(fill: if dark { white } else { black })\nFlow integration fixture\n')
        with socket.socket() as reservation:
            reservation.bind(('127.0.0.1', 0)); port = reservation.getsockname()[1]
        data = self.root / 'data'; data.mkdir(exist_ok=True)
        origin = 'https://preview.example'
        profile = {'id': 'fixture', 'name': 'Fixture', 'root': str(project), 'entry': 'main.typ', 'fonts': [],
                   'packages': '', 'port': port, 'publicURL': origin, 'label': 'com.example.flow.fixture'}
        (data / 'profiles.json').write_text(json.dumps({'schemaVersion': 1, 'selectedID': 'fixture', 'profiles': [profile]}))
        with (self.root / 'preview.log').open('w+') as log:
            child = subprocess.Popen([package.executable(self.app), '--serve', 'fixture'], env=dict(os.environ, **self.env),
                                     stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline:
                    if child.poll() is not None:
                        log.seek(0); self.fail('Preview exited: ' + log.read()[-4000:])
                    try:
                        with urllib.request.urlopen(f'http://127.0.0.1:{port}/', timeout=1) as response:
                            html = response.read()
                        break
                    except OSError: time.sleep(.1)
                else: self.fail('Preview did not start')
                self.assertIn(b'preview-arg:systemTheme:true', html)
                light = document_frame(port, '/_theme/light', origin)
                dark = document_frame(port, '/_theme/dark', origin)
                self.assertNotEqual(hashlib.sha256(light).digest(), hashlib.sha256(dark).digest())
            finally:
                if child.poll() is None: os.killpg(child.pid, signal.SIGTERM)
                try: child.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL); child.wait()

    @unittest.skipUnless(os.environ.get('FLOW_TEST_SIGN_IDENTITY'), 'Set FLOW_TEST_SIGN_IDENTITY for persistent-certificate checks')
    def test_changed_bundle_reuses_saved_identity_and_designated_requirements(self):
        identity = os.environ['FLOW_TEST_SIGN_IDENTITY']
        changed = self.root / 'updated/tinymist-flow.app'
        with patch.object(package, 'DATA', self.root / 'signing-data'), patch.dict(os.environ, FLOW_SIGN_IDENTITY=''):
            package.configure_signing(argparse.Namespace(identity=identity))
            configured_identity = package.resolve_signing_identity()
            package.build(argparse.Namespace(engine=self.engine, output=changed, version='0.1.1', identity=None))
        manifest = package.validate(changed)
        self.assertEqual(manifest['signing'], configured_identity)
        self.assertEqual(manifest['signingCertificateSHA256'], package.validate(self.app)['signingCertificateSHA256'])
        self.assertNotEqual(package.digest(package.executable(self.app)), package.digest(package.executable(changed)))
        for old, new in ((self.app, changed), (self.app / 'Contents/MacOS/flow-engine', changed / 'Contents/MacOS/flow-engine')):
            output = subprocess.check_output(['/usr/bin/codesign', '-d', '-r-', old], stderr=subprocess.STDOUT, text=True)
            requirement = output.split('designated => ', 1)[1].strip()
            self.assertNotIn('cdhash', requirement)
            subprocess.run(['/usr/bin/codesign', '--verify', '--strict', '-R', '=' + requirement, new], check=True, capture_output=True)
            output = subprocess.check_output(['/usr/bin/codesign', '-d', '-r-', new], stderr=subprocess.STDOUT, text=True)
            self.assertEqual(output.split('designated => ', 1)[1].strip(), requirement)


if __name__ == '__main__': unittest.main()
