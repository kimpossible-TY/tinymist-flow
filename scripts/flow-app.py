#!/usr/bin/env python3
"""Build, install, update, roll back, and migrate the personal macOS app."""
from __future__ import annotations
import argparse
import datetime as dt
import fcntl
import hashlib
import json
import os
from pathlib import Path
import plistlib
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
APP_ID = 'io.github.kimpossible-ty.tinymist-flow'
DEFAULT_APP = Path.home() / 'Applications/tinymist-flow.app'
DATA = Path.home() / 'Library/Application Support/tinymist-flow'

def run(args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)

def capture(args, **kwargs):
    return subprocess.check_output([str(a) for a in args], text=True, **kwargs).strip()

def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix='.' + path.name)
    try:
        with os.fdopen(fd, 'w') as f:
            json.dump(value, f, indent=2, ensure_ascii=False); f.write('\n')
        os.replace(tmp, path)
    finally:
        Path(tmp).unlink(missing_ok=True)

def executable(app):
    return app / 'Contents/MacOS/tinymist-flow'

def signing_identities():
    output = capture(['/usr/bin/security', 'find-identity', '-p', 'codesigning'])
    return dict(re.findall(r'\)\s+([0-9A-F]{40})\s+"([^"]+)"', output))

def resolve_signing_identity(requested=None):
    override = requested or os.environ.get('FLOW_SIGN_IDENTITY')
    if override: return override
    path = DATA / 'signing.json'
    if not path.exists(): return '-'
    config = json.loads(path.read_text())
    if not isinstance(config, dict):
        raise ValueError('Invalid signing configuration: ' + str(path))
    identity = config.get('identity', '')
    if config.get('schemaVersion') != 1 or not isinstance(identity, str) or not re.fullmatch('[0-9A-F]{40}', identity):
        raise ValueError('Invalid signing configuration: ' + str(path))
    if identity not in signing_identities():
        raise ValueError('Configured signing certificate is unavailable. Restore its Keychain identity '
                         'or explicitly run configure-signing; refusing ad-hoc fallback.')
    return identity

def signing_certificate(path):
    """Return public certificate fingerprints without exporting its private key."""
    with tempfile.TemporaryDirectory(prefix='flow-cert-') as tmp:
        prefix = Path(tmp) / 'certificate'
        run(['/usr/bin/codesign', '--display', '--extract-certificates=' + str(prefix), path], capture_output=True)
        leaf = Path(str(prefix) + '0')
        if not leaf.exists(): return None
        data = leaf.read_bytes()
        return {'sha1': hashlib.sha1(data).hexdigest().upper(), 'sha256': hashlib.sha256(data).hexdigest()}

def configure_signing(args):
    """Save a verified existing Keychain identity for subsequent local builds."""
    identities = signing_identities()
    matches = [sha for sha, name in identities.items() if args.identity.upper() == sha or args.identity == name]
    if len(matches) != 1:
        raise ValueError('Select exactly one available Keychain code-signing identity by name or SHA-1.')
    identity = matches[0]
    with tempfile.TemporaryDirectory(prefix='flow-signing-check-') as tmp:
        probe = Path(tmp) / 'probe'; shutil.copyfile('/usr/bin/true', probe); probe.chmod(0o755)
        run(['/usr/bin/codesign', '--force', '--sign', identity, '--identifier', APP_ID, probe], capture_output=True)
        run(['/usr/bin/codesign', '--verify', '--strict', probe], capture_output=True)
        certificate = signing_certificate(probe)
        if not certificate or certificate['sha1'] != identity:
            raise ValueError('Signing probe did not use the selected certificate')
    path = DATA / 'signing.json'
    write_json(path, {'schemaVersion': 1, 'identity': identity, 'name': identities[identity]})
    path.chmod(0o600)
    print('Signing identity saved: ' + identities[identity] + ' (' + identity + ')')

def validate(app):
    info = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
    if info.get('CFBundleIdentifier') != APP_ID or info.get('CFBundleExecutable') != 'tinymist-flow':
        raise ValueError('Not a tinymist-flow app bundle')
    run(['/usr/bin/codesign', '--verify', '--deep', '--strict', app], capture_output=True)
    manifest = json.loads((app / 'Contents/Resources/release.json').read_text())
    certificate_hash = manifest.get('signingCertificateSHA256')
    if certificate_hash:
        for path in (app, app / 'Contents/MacOS/flow-engine'):
            certificate = signing_certificate(path)
            if not certificate or certificate['sha256'] != certificate_hash:
                raise ValueError('Signing certificate mismatch: ' + str(path))
    if digest(app / 'Contents/MacOS/flow-engine') != manifest['engineSHA256']:
        raise ValueError('Bundled engine hash mismatch')
    if capture([executable(app), '--version']) != 'tinymist-flow ' + manifest['version']:
        raise ValueError('Version mismatch')
    run([app / 'Contents/MacOS/flow-engine', '--version'], stdout=subprocess.DEVNULL)
    return manifest

def resolve_engine(requested=None):
    selected = requested or os.environ.get('FLOW_ENGINE_PATH') or DEFAULT_APP / 'Contents/MacOS/flow-engine'
    engine = Path(selected).expanduser().resolve()
    if not engine.is_file() or not os.access(engine, os.X_OK):
        raise ValueError('A compatible external engine is required. Pass --engine /path/to/flow-engine '
                         'or set FLOW_ENGINE_PATH; the installed Flow engine is the default.')
    version = capture([engine, '--version'])
    if '--follow-system-theme' not in capture([engine, 'preview', '--help']):
        raise ValueError('The external engine must support Flow preview features, including '
                         '--follow-system-theme. Reuse a compatible installed Flow engine.')
    return engine, version

def build(args):
    engine_build = None
    if getattr(args, 'build_engine', False):
        args.engine, engine_build = build_native_engine(args.jobs)
    engine, engine_version = resolve_engine(args.engine)
    app = args.output.expanduser().resolve()
    if app.name != 'tinymist-flow.app': raise ValueError('Unexpected output app name')
    app.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.flow-build-', dir=app.parent) as tmp:
        staged = Path(tmp) / app.name
        manifest = build_bundle(args, staged, engine, engine_version, engine_build)
        if app.exists(): shutil.rmtree(app)
        os.replace(staged, app)
    write_json(app.parent / 'release.json', {**manifest, 'appExecutableSHA256': digest(executable(app))})
    print(app)

def build_bundle(args, app, engine, engine_version, engine_build=None):
    identity = resolve_signing_identity(getattr(args, 'identity', None))
    mac = app / 'Contents/MacOS'; resources = app / 'Contents/Resources'
    mac.mkdir(parents=True); resources.mkdir()
    engine_input_hash = digest(engine)
    shutil.copy2(engine, mac / 'flow-engine')
    run(['/usr/bin/swiftc', '-swift-version', '5', '-warnings-as-errors', '-O',
         ROOT / 'apps/macos/FlowCore.swift', ROOT / 'apps/macos/main.swift', '-o', mac / 'tinymist-flow'])
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / 'Flow.iconset'; iconset.mkdir()
        for size in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                filename = f'icon_{size}x{size}' + ('@2x' if scale == 2 else '') + '.png'
                run(['/usr/bin/sips', '-z', size * scale, size * scale, ROOT / 'assets/branding/tinymist-flow.png', '--out', iconset / filename], stdout=subprocess.DEVNULL)
        run(['/usr/bin/iconutil', '-c', 'icns', iconset, '-o', resources / 'Flow.icns'])
    version = args.version
    if not __import__('re').fullmatch(r'\d+\.\d+\.\d+', version): raise ValueError('Version must be X.Y.Z')
    info = {
        'CFBundleIdentifier': APP_ID, 'CFBundleName': 'tinymist-flow', 'CFBundleDisplayName': 'tinymist-flow',
        'CFBundleExecutable': 'tinymist-flow', 'CFBundleIconFile': 'Flow', 'CFBundlePackageType': 'APPL',
        'CFBundleShortVersionString': version, 'CFBundleVersion': version, 'LSMinimumSystemVersion': '13.0',
        'LSUIElement': True, 'NSHighResolutionCapable': True,
        'NSDocumentsFolderUsageDescription': '선택한 Typst 프로젝트와 글꼴을 읽고 실시간 프리뷰를 제공합니다.',
        'NSDesktopFolderUsageDescription': '선택한 Typst 프로젝트를 읽고 실시간 프리뷰를 제공합니다.',
        'NSDownloadsFolderUsageDescription': '선택한 Typst 프로젝트를 읽고 실시간 프리뷰를 제공합니다.',
        'NSHumanReadableCopyright': 'Tinymist contributors and tinymist-flow contributors. Apache License 2.0.',
    }
    (app / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
    shutil.copy2(ROOT / 'LICENSE', resources / 'LICENSE')
    guide = ROOT / 'docs/tinymist/flow-app.typ'
    if guide.exists():
        run([engine, 'compile', guide, resources / 'UserGuide.pdf', '--root', ROOT])
    run(['/usr/bin/codesign', '--force', '--sign', identity, '--identifier', APP_ID + '.engine', mac / 'flow-engine'])
    manifest = {'version': version, 'sourceRevision': capture(['git', '-C', ROOT, 'rev-parse', 'HEAD']),
                'sourceDirty': bool(capture(['git', '-C', ROOT, 'status', '--porcelain'])),
                'engineSource': 'native' if engine_build is not None else 'external', 'engineVersion': engine_version,
                'engineInputSHA256': engine_input_hash,
                'engineSHA256': digest(mac / 'flow-engine'), 'signing': 'ad-hoc' if identity == '-' else identity,
                'builtAt': dt.datetime.now(dt.timezone.utc).isoformat()}
    if identity != '-':
        certificate = signing_certificate(mac / 'flow-engine')
        if not certificate: raise ValueError('Certificate signing produced no certificate')
        manifest['signingCertificateSHA256'] = certificate['sha256']
    if engine_build is not None:
        manifest['engineBuild'] = engine_build
    write_json(resources / 'release.json', manifest)
    run(['/usr/bin/codesign', '--force', '--sign', identity, '--identifier', APP_ID, app])
    validate(app)
    return manifest

def build_native_engine(jobs):
    """Build a portable native Mac engine with a recorded ThinLTO profile."""
    if sys.platform != 'darwin':
        raise ValueError('The macOS app must be built on macOS')
    if jobs < 1:
        raise ValueError('Build jobs must be positive')
    machine = platform.machine()
    if machine == 'x86_64':
        translated = subprocess.run(['/usr/sbin/sysctl', '-in', 'sysctl.proc_translated'],
                                    capture_output=True, text=True)
        if translated.stdout.strip() == '1':
            raise ValueError('Run the build with a native ARM Python or terminal on Apple Silicon')
    target = {'arm64': 'aarch64-apple-darwin', 'x86_64': 'x86_64-apple-darwin'}.get(machine)
    if target is None:
        raise ValueError('Unsupported macOS host architecture')
    metadata = json.loads(capture(['cargo', 'metadata', '--locked', '--format-version=1', '--no-deps',
                                   '--manifest-path', ROOT / 'Cargo.toml'], cwd=ROOT))
    run(['cargo', 'build', '--locked', '--profile', 'flow-release', '--target', target,
         '--bin', 'tinymist', '--jobs', str(jobs)], cwd=ROOT)
    path = Path(metadata['target_directory']) / target / 'flow-release/tinymist'
    settings = {'profile': 'flow-release', 'target': target, 'configuredLto': 'thin',
                'rustflags': os.environ.get('RUSTFLAGS', ''),
                'encodedRustflags': os.environ.get('CARGO_ENCODED_RUSTFLAGS', '')}
    if 'CARGO_PROFILE_FLOW_RELEASE_LTO' in os.environ:
        settings['profileLtoOverride'] = os.environ['CARGO_PROFILE_FLOW_RELEASE_LTO']
    return path, settings

def profiles(app):
    if not executable(app).exists(): return []
    return json.loads(capture([executable(app), '--profiles']))['profiles']

def running(app):
    active = []
    for p in profiles(app):
        state = json.loads(capture([executable(app), '--status', p['id']]))
        if state['running'] and state.get('managedByApp', False): active.append(p['id'])
    return active

def healthy(app, ids, timeout=30):
    if not ids: return
    by_id = {p['id']: p for p in profiles(app)}
    deadline = time.monotonic() + timeout
    pending = set(ids)
    while pending and time.monotonic() < deadline:
        for pid in list(pending):
            try:
                state = json.loads(capture([executable(app), '--status', pid]))
                with urllib.request.urlopen(f'http://127.0.0.1:{by_id[pid]["port"]}/', timeout=1) as response:
                    if state['running'] and response.status == 200: pending.remove(pid)
            except Exception: pass
        if pending: time.sleep(.5)
    if pending: raise RuntimeError('Preview startup failed; inspect Documents consent and logs: ' + ', '.join(pending))

def replace_app(source, destination, data_dir=DATA, check_health=True):
    data_dir.mkdir(parents=True, exist_ok=True)
    with (data_dir / 'install.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        return _replace_app(source, destination, data_dir, check_health)

def _replace_app(source, destination, data_dir, check_health):
    """Validate before stopping services; restore the old bundle on a failed update."""
    validate(source)
    destination.parent.mkdir(parents=True, exist_ok=True)
    stamp = dt.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    releases = data_dir / 'releases'; releases.mkdir(parents=True, exist_ok=True)
    backup = releases / stamp / 'tinymist-flow.app'
    active = running(destination)
    stage = Path(tempfile.mkdtemp(prefix='.flow-install-', dir=destination.parent)) / 'tinymist-flow.app'
    shutil.copytree(source, stage, symlinks=True)
    validate(stage)
    stopped = []
    had_previous = destination.exists()
    swapped = False
    try:
        for pid in active:
            run([executable(destination), '--control', 'stop', pid]); stopped.append(pid)
        if had_previous:
            backup.parent.mkdir(); os.replace(destination, backup)
        os.replace(stage, destination); swapped = True
        for pid in active: run([executable(destination), '--control', 'start', pid])
        if check_health: healthy(destination, active)
    except Exception:
        for pid in stopped:
            if executable(destination).exists(): subprocess.run([str(executable(destination)), '--control', 'stop', pid], capture_output=True)
        if swapped and destination.exists():
            failed = releases / (stamp + '-failed'); failed.mkdir(); os.replace(destination, failed / destination.name)
        if backup.exists(): os.replace(backup, destination)
        for pid in stopped: subprocess.run([str(executable(destination)), '--control', 'start', pid], capture_output=True)
        raise
    finally:
        shutil.rmtree(stage.parent, ignore_errors=True)
    if had_previous: write_json(data_dir / 'previous-release.json', {'app': str(backup), 'replacedAt': stamp})
    write_json(data_dir / 'installed-release.json', validate(destination))
    print(destination)

def install(args):
    deferred = getattr(args, 'defer_health_check', False)
    replace_app(args.source.resolve(), args.app.expanduser().resolve(), check_health=not deferred)
    if deferred:
        print('Live preview health verification deferred; complete any macOS consent prompt, then verify document delivery.')

def rollback(args):
    previous = Path(json.loads((DATA / 'previous-release.json').read_text())['app'])
    replace_app(previous, args.app.expanduser().resolve())

def migrate(args):
    root = args.root.resolve(); app = args.app.expanduser().resolve(); validate(app)
    registry = root / 'config/services.json'; script = root / 'bin/tail-hosting'
    backup = DATA / 'migration-backup'
    backup.mkdir(parents=True, exist_ok=True)
    # Never overwrite the original pre-migration recovery files.
    for path, name in [(registry, 'services.json'), (script, 'tail-hosting')]:
        if not (backup / name).exists(): shutil.copy2(path, backup / name)
    config_path = DATA / 'profiles.json'
    if config_path.exists() and not (backup / 'profiles.json').exists(): shutil.copy2(config_path, backup / 'profiles.json')
    config = json.loads(config_path.read_text()) if config_path.exists() else {'schemaVersion': 1, 'selectedID': '', 'profiles': []}
    data = json.loads(registry.read_text())
    for service in data['services']:
        if service.get('kind') != 'typst-always-preview': continue
        sid = service['id']; label = service['launchAgentLabel']
        plist = Path.home() / 'Library/LaunchAgents' / (label + '.plist')
        if plist.exists() and not (backup / plist.name).exists(): shutil.copy2(plist, backup / plist.name)
        ingress = service['ingress']; host = data['host']['dnsName']
        public = f'https://{host}:{ingress["port"]}{ingress.get("path", "/")}'
        profile = {'id': sid, 'name': service['name'], 'root': service['projectRoot'], 'entry': service.get('entrypoint', 'main.typ'),
                   'fonts': service.get('fontPaths', []), 'packages': str(Path.home() / 'Library/Application Support/typst/packages'),
                   'port': service['backend']['port'], 'publicURL': public, 'label': label}
        if not any(p['id'] == sid for p in config['profiles']): config['profiles'].append(profile)
        if not config['selectedID']: config['selectedID'] = sid
        service['managerBinary'] = str(executable(app))
        service['enginePath'] = str(app / 'Contents/MacOS/flow-engine')
        service['focusFile'] = str(DATA / 'focus' / (sid + '.json'))
    write_json(config_path, config)
    run([executable(app), '--check-config'])
    patch = ROOT / 'apps/macos/tail-hosting.patch'
    if 'Flow app lifecycle adapter' not in script.read_text():
        run(['/usr/bin/patch', '--dry-run', '-p1', '-i', patch], cwd=root)
        run(['/usr/bin/patch', '-p1', '-i', patch], cwd=root)
    write_json(registry, data)
    write_json(backup / 'migration.json', {'hostingRoot': str(root), 'app': str(app)})
    print('Profiles imported; ingress preserved. Activate with the app Restart control.')
    print('Recovery files:', backup)

def restore_hosting(args):
    backup = DATA / 'migration-backup'
    saved = json.loads((backup / 'migration.json').read_text()); root = Path(saved['hostingRoot'])
    app = Path(saved['app'])
    original = json.loads((backup / 'services.json').read_text())
    for s in original['services']:
        if s.get('kind') != 'typst-always-preview': continue
        subprocess.run([str(executable(app)), '--control', 'stop', s['id']], capture_output=True)
        label = s['launchAgentLabel']; plist = Path.home() / 'Library/LaunchAgents' / (label + '.plist')
        shutil.copy2(backup / plist.name, plist)
    shutil.copy2(backup / 'services.json', root / 'config/services.json')
    shutil.copy2(backup / 'tail-hosting', root / 'bin/tail-hosting')
    for s in original['services']:
        if s.get('kind') == 'typst-always-preview' and s.get('desiredState') == 'running': run([root / 'bin/tail-hosting', 'start', s['id']])
    print('Previous Tail Hosting configuration restored.')

def main():
    parser = argparse.ArgumentParser(description=__doc__); sub = parser.add_subparsers(dest='command', required=True)
    b = sub.add_parser('build'); b.add_argument('--engine', type=Path, help='External Flow-compatible engine (FLOW_ENGINE_PATH or installed app by default)'); b.add_argument('--output', type=Path, default=ROOT / 'dist/tinymist-flow.app'); b.add_argument('--version', default='0.1.0'); b.add_argument('--identity', help='Signing identity (FLOW_SIGN_IDENTITY or saved configure-signing identity by default)'); b.add_argument('--build-engine', action='store_true', help='Build the native engine with ThinLTO before packaging'); b.add_argument('--jobs', type=int, default=2, help='Cargo workers when building the engine'); b.set_defaults(func=build)
    s = sub.add_parser('configure-signing'); s.add_argument('--identity', required=True, help='Existing Keychain code-signing certificate name or SHA-1'); s.set_defaults(func=configure_signing)
    i = sub.add_parser('install'); i.add_argument('source', type=Path); i.add_argument('--app', type=Path, default=DEFAULT_APP); i.add_argument('--defer-health-check', action='store_true', help='Defer the live HTTP deadline while macOS consent is pending; keep bundle validation and backup'); i.set_defaults(func=install)
    r = sub.add_parser('rollback'); r.add_argument('--app', type=Path, default=DEFAULT_APP); r.set_defaults(func=rollback)
    m = sub.add_parser('migrate-tail-hosting'); m.add_argument('root', type=Path); m.add_argument('--app', type=Path, default=DEFAULT_APP); m.set_defaults(func=migrate)
    restore = sub.add_parser('restore-tail-hosting'); restore.set_defaults(func=restore_hosting)
    v = sub.add_parser('verify'); v.add_argument('app', type=Path); v.set_defaults(func=lambda a: print(json.dumps(validate(a.app), indent=2)))
    args = parser.parse_args()
    try: args.func(args)
    except (ValueError, RuntimeError, OSError, subprocess.CalledProcessError) as e: print(str(e), file=sys.stderr); sys.exit(1)

if __name__ == '__main__': main()
