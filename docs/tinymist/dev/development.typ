= Flow development guide

This repository owns the native tinymist-flow app, project profiles, preview lifecycle, packaging, and documentation. Compilation and preview rendering are supplied by a compatible external tinymist executable.

= Layout

- `apps/macos/FlowCore.swift`: configuration validation, LaunchAgents, CLI, and engine launch.
- `apps/macos/main.swift`: AppKit menu bar and project window.
- `apps/macos/tail-hosting.patch`: optional lifecycle adapter for Tail Hosting.
- `scripts/flow-app.py`: staged app builds, signing, installation, rollback, and migration.
- `tests/flow-app/`: app and packaging tests.
- `assets/branding/`: app branding.
- `docs/tinymist/`: canonical Typst sources.
- `openspec/changes/package-tinymist-flow-app/`: current app contracts and work items.

= Engine input

Keep engine executables outside the managed source tree. Packaging chooses `--engine` first, then `FLOW_ENGINE_PATH`, then `~/Applications/tinymist-flow.app/Contents/MacOS/flow-engine`. The build copies and signs the supplied executable; it never builds engine sources. The engine version and input hash identify the dependency independently of the Flow source commit.

A compatible engine supports document preview, device theme variants, `TINYMIST_PREVIEW_FOCUS_FILE`, `TINYMIST_PREVIEW_CHANGE_FILE`, and `TINYMIST_ALLOWED_ORIGINS`. The CLI probe requires `--follow-system-theme`; retain the current installed Flow engine when preserving these features. A different engine needs compatibility verification before use.

= Build and validate

macOS, the Swift compiler, and Python 3 are required for the app. AppKit and Foundation provide the runtime.

```sh
python3 scripts/flow-app.py build --engine /absolute/path/to/flow-engine
python3 -m unittest discover -s tests/flow-app -v
python3 scripts/flow-app.py verify dist/tinymist-flow.app
```

Tests compile the current Swift sources with warnings treated as errors. Test profiles and update-recovery fixtures use temporary directories. Optional packaging and preview integration tests use `FLOW_TEST_ENGINE` to select an external executable.

Build into `dist`; use the installer for an installed app. Keep installed engines independent of build output. Verify actual document delivery after a release installation, then tag confirmed versions as `flow-vX.Y.Z`.

= Documentation

Edit Typst sources, then regenerate the Markdown outputs. The generator uses an external `typlite` executable from PATH or `TYPLITE_BIN`. Prebuilt typlite executables are available from the #link("https://github.com/Myriad-Dreamin/tinymist/releases")[Tinymist releases]. Node.js is only needed for this generator and the optional package-script shortcuts.

```sh
node scripts/link-docs.mjs
node scripts/link-docs.mjs --check
typst compile --root . docs/tinymist/flow-app.typ dist/UserGuide.pdf
```

`README.md` and `docs/dev-guide.md` are generated. The app guide is bundled as a PDF during packaging. App sources compile independently of documentation conversion tools.

= Workflow

Use a short feature branch and Conventional Commits. Read the matching OpenSpec change before changing behavior and keep its artifacts aligned. Run the app tests and checks for the files touched. Prior engine implementation and deployment records remain in Git history; ongoing work belongs to the Flow app.
