# AGENTS.md

This repository is the tinymist-flow macOS app. Flow manages Typst project settings, preview services, and recoverable installation. It consumes a compatible external tinymist executable; engine implementation and editor extension sources are outside this repository's maintained scope.

## Quick start

1. Read `docs/dev-guide.md` and the affected app, script, or test before editing.
2. Check `openspec/config.yaml`, accepted specs, and matching active changes for behavior changes and refactors. Continue matching work rather than creating parallel artifacts.
3. Preserve unrelated user changes. Use short feature branches and Conventional Commits.
4. Run the smallest meaningful validation for the files touched and report its results.

## Source ownership

- `apps/macos/`: Swift/AppKit app, service CLI, and Tail Hosting adapter.
- `assets/branding/`: app icon and branding.
- `scripts/flow-app.py`: staged builds, signing, installation, rollback, and migration.
- `tests/flow-app/`: freshly compiled app configuration and packaging tests.
- `docs/tinymist/`: canonical Typst documentation; developer sources live under `dev/`.
- `openspec/`: Flow app behavior and workflow contracts.

Keep engine binaries and generated bundles outside the managed source tree. Build accepts `--engine`, `FLOW_ENGINE_PATH`, or the current installed Flow engine. Preserve the engine's preview CLI and environment contract unless the task explicitly changes it. Record external engine provenance independently of the Flow source revision.

## Documentation

Do not hand-edit generated Markdown. `README.md` and `docs/dev-guide.md` come from Typst sources via `node scripts/link-docs.mjs`. This uses an external typlite executable from PATH or `TYPLITE_BIN`. Edit the source and regenerate the outputs. The app bundles `docs/tinymist/flow-app.typ` as its user guide.

Keep OpenSpec proposal, design, tasks, and spec deltas aligned with implementation. Use archived work and earlier Git history as prior art rather than current requirements.

## Validation

- Native app and packaging: `python3 -m unittest discover -s tests/flow-app -v`
- Bundle build: `python3 scripts/flow-app.py build --engine /path/to/flow-engine`
- Signature, hashes, and version: `python3 scripts/flow-app.py verify dist/tinymist-flow.app`
- Optional integration: set `FLOW_TEST_ENGINE` when running the app tests.
- Generated docs: `node scripts/link-docs.mjs --check`
- Typst guide: `typst compile --root . docs/tinymist/flow-app.typ dist/UserGuide.pdf`

Compile Swift with warnings treated as errors. App tests use temporary profiles and must not change live projects, LaunchAgents, or the installed app.

## Release and maintenance

Build releases from a clean commit. Install a verified bundle with the installer, verify document delivery, and tag confirmed versions `flow-vX.Y.Z`. Keep a recovery bundle until the update is confirmed. Tail Hosting owns HTTPS ingress and remote access policy; Flow owns app profiles and preview lifecycle.

Preserve Apache License 2.0 and applicable upstream notices. Do not change release versions or engine selection unless the task requires it. Commit messages follow Conventional Commits. PR content is limited to modified features and issue operations, with no execution transcripts or validation logs.
