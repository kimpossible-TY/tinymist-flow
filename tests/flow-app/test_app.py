"""Configuration contract and failed-update recovery, without touching live jobs."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('flow_package', ROOT / 'scripts/flow-app.py')
package = importlib.util.module_from_spec(spec); spec.loader.exec_module(package)

class Profiles(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if sys.platform != 'darwin': raise unittest.SkipTest('Native app tests require macOS')
        temp = tempfile.TemporaryDirectory(prefix='flow-app-tests-'); cls.addClassCleanup(temp.cleanup)
        cls.app = Path(temp.name) / 'tinymist-flow'
        subprocess.run(['/usr/bin/swiftc', '-swift-version', '5', '-warnings-as-errors',
                        ROOT / 'apps/macos/FlowCore.swift', ROOT / 'apps/macos/main.swift', '-o', cls.app], check=True)
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = dict(os.environ, FLOW_DATA_DIR=str(self.root / 'data'), FLOW_LOG_DIR=str(self.root / 'logs'))
        self.profile = {'id': 'test', 'name': 'A project with spaces', 'root': str(self.root / 'project with spaces'),
                        'entry': 'main.typ', 'fonts': ['fonts'], 'packages': '', 'port': 24650,
                        'publicURL': 'https://example.ts.net:24650/', 'label': 'com.example.flow.test'}
    def config(self, profiles=None):
        config = {'schemaVersion': 1, 'selectedID': 'test', 'profiles': profiles if profiles is not None else [self.profile]}
        path = self.root / 'data/profiles.json'; path.parent.mkdir(exist_ok=True); path.write_text(json.dumps(config))
    def cli(self, *args):
        return subprocess.run([self.app, *args], env=self.env, text=True, capture_output=True)
    def test_arguments_and_identity(self):
        self.config(); result = self.cli('--launch-plan', 'test'); self.assertEqual(result.returncode, 0, result.stderr)
        plan = json.loads(result.stdout)
        self.assertIn('--data-plane-host=127.0.0.1:24650', plan['arguments'])
        self.assertIn(str(self.root / 'project with spaces/main.typ'), plan['arguments'])
        self.assertIn('--no-open', plan['arguments'])
        self.assertIn('--follow-system-theme', plan['arguments'])
        self.assertEqual(plan['environment']['TINYMIST_ALLOWED_ORIGINS'], 'https://example.ts.net:24650')
        self.assertEqual(plan['agent']['ProgramArguments'][1:], ['--serve', 'test'])
        self.assertIn(package.APP_ID, plan['agent']['AssociatedBundleIdentifiers'])
        self.assertTrue(plan['environment']['TINYMIST_PREVIEW_FOCUS_FILE'].endswith('/data/focus/test.json'))
        self.assertTrue(plan['environment']['TINYMIST_PREVIEW_CHANGE_FILE'].endswith('/data/changes/test.json'))
    def test_duplicate_port_rejected(self):
        second = dict(self.profile, id='second', label='com.example.second')
        self.config([self.profile, second]); self.assertNotEqual(self.cli('--check-config').returncode, 0)
    def test_invalid_origins_and_entry(self):
        for url in ['http://example.com', 'https://user:pass@example.com', 'https://example.com/path', 'https://example.com/?query=1']:
            with self.subTest(url=url):
                self.profile['publicURL'] = url; self.config(); self.assertNotEqual(self.cli('--check-config').returncode, 0)
        self.profile['publicURL'] = ''; self.profile['entry'] = '../outside.typ'; self.config()
        self.assertNotEqual(self.cli('--check-config').returncode, 0)
    def test_bad_id_and_port_rejected(self):
        self.profile['id'] = '../escape'; self.config(); self.assertNotEqual(self.cli('--check-config').returncode, 0)
        self.profile['id'] = 'test'; self.profile['port'] = 0; self.config(); self.assertNotEqual(self.cli('--check-config').returncode, 0)
    def test_missing_project_does_not_start_job(self):
        self.config(); result = self.cli('--control', 'start', 'test')
        self.assertNotEqual(result.returncode, 0); self.assertIn('진입 파일', result.stderr)

class EngineInput(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(); self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.engine = self.root / 'external engine'
        self.engine.write_text('engine'); self.engine.chmod(0o755)

    def test_explicit_engine_takes_precedence_over_environment(self):
        with patch.dict(os.environ, FLOW_ENGINE_PATH='/missing/engine'), patch.object(package, 'capture', side_effect=['tinymist test', '--follow-system-theme']):
            engine, version = package.resolve_engine(self.engine)
        self.assertEqual(engine, self.engine); self.assertEqual(version, 'tinymist test')

    def test_environment_and_installed_engine_sources(self):
        with patch.dict(os.environ, FLOW_ENGINE_PATH=str(self.engine)), patch.object(package, 'capture', side_effect=['tinymist test', '--follow-system-theme']):
            self.assertEqual(package.resolve_engine()[0], self.engine)
        installed = self.root / 'installed.app'; binary = installed / 'Contents/MacOS/flow-engine'
        binary.parent.mkdir(parents=True); binary.write_text('engine'); binary.chmod(0o755)
        with patch.dict(os.environ, {}, clear=True), patch.object(package, 'DEFAULT_APP', installed), patch.object(package, 'capture', side_effect=['tinymist test', '--follow-system-theme']):
            self.assertEqual(package.resolve_engine()[0], binary)

    def test_invalid_engine_keeps_previous_output(self):
        output = self.root / 'tinymist-flow.app'; output.mkdir(); marker = output / 'previous'; marker.write_text('preserved')
        args = argparse.Namespace(engine=self.root / 'missing', output=output)
        with self.assertRaisesRegex(ValueError, 'external engine'): package.build(args)
        self.assertEqual(marker.read_text(), 'preserved')
        args.engine = self.engine
        with patch.object(package, 'capture', side_effect=['tinymist test', 'standard preview']), self.assertRaisesRegex(ValueError, 'Flow preview features'):
            package.build(args)
        self.assertEqual(marker.read_text(), 'preserved')

    def test_failed_bundle_build_keeps_previous_output(self):
        output = self.root / 'tinymist-flow.app'; output.mkdir(); marker = output / 'previous'; marker.write_text('preserved')
        args = argparse.Namespace(engine=self.engine, output=output)
        with patch.object(package, 'resolve_engine', return_value=(self.engine, 'tinymist test')), patch.object(package, 'build_bundle', side_effect=RuntimeError('build failed')):
            with self.assertRaisesRegex(RuntimeError, 'build failed'): package.build(args)
        self.assertEqual(marker.read_text(), 'preserved')

class InstallRecovery(unittest.TestCase):
    def test_update_does_not_take_over_legacy_service(self):
        with patch.object(package, 'profiles', return_value=[{'id': 'test'}]), patch.object(package, 'capture', return_value=json.dumps({'running': True, 'managedByApp': False})):
            self.assertEqual(package.running(Path('/fake/app')), [])
        with patch.object(package, 'profiles', return_value=[{'id': 'test'}]), patch.object(package, 'capture', return_value=json.dumps({'running': True, 'managedByApp': True})):
            self.assertEqual(package.running(Path('/fake/app')), ['test'])
    def test_failed_update_restores_previous_bundle(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); old = root / 'Applications/tinymist-flow.app'; incoming = root / 'new/tinymist-flow.app'
            for path, text in [(old, 'old'), (incoming, 'new')]:
                package.executable(path).parent.mkdir(parents=True); package.executable(path).write_text(text)
            settings = root / 'data/profiles.json'; settings.parent.mkdir(); settings.write_text('preserved')
            with patch.object(package, 'validate', return_value={}), patch.object(package, 'running', return_value=['test']), patch.object(package, 'run'), patch.object(package.subprocess, 'run'), patch.object(package, 'healthy', side_effect=RuntimeError('failed')):
                with self.assertRaises(RuntimeError): package.replace_app(incoming, old, root / 'data')
            self.assertEqual(package.executable(old).read_text(), 'old')
            self.assertEqual(settings.read_text(), 'preserved')
    def test_successful_update_keeps_recovery_bundle(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); old = root / 'Applications/tinymist-flow.app'; incoming = root / 'new/tinymist-flow.app'
            for path, text in [(old, 'old'), (incoming, 'new')]:
                package.executable(path).parent.mkdir(parents=True); package.executable(path).write_text(text)
            with patch.object(package, 'validate', return_value={'version': 'test'}), patch.object(package, 'running', return_value=[]):
                package.replace_app(incoming, old, root / 'data')
            previous = Path(json.loads((root / 'data/previous-release.json').read_text())['app'])
            self.assertEqual(package.executable(previous).read_text(), 'old')
            self.assertEqual(package.executable(old).read_text(), 'new')

if __name__ == '__main__': unittest.main()
