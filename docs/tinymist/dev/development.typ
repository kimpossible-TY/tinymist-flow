= Flow development guide

This repository owns the native tinymist-flow app, project profiles, preview lifecycle, packaging, and documentation. Compilation and preview rendering are supplied by the maintained tinymist engine or a compatible external executable.

= Layout

- `apps/macos/FlowCore.swift`: configuration validation, LaunchAgents, CLI, and engine launch.
- `apps/macos/main.swift`: AppKit menu bar and project window.
- `apps/macos/tail-hosting.patch`: optional lifecycle adapter for Tail Hosting.
- `scripts/flow-app.py`: staged app builds, signing, installation, rollback, and migration.
- `tests/flow-app/`: app and packaging tests.
- `assets/branding/`: app branding.
- `docs/tinymist/`: canonical Typst sources.
- `openspec/changes/package-tinymist-flow-app/`: current app contracts and work items.
- `crates/`: engine implementations and supporting libraries.
- `tools/typst-preview-frontend/` and `tools/typst-dom/`: preview frontend and document rendering.
- `editors/`: editor integrations and tooling.

= Engine input

Keep engine executables outside the managed source tree. Packaging chooses `--engine` first, then `FLOW_ENGINE_PATH`, then `~/Applications/tinymist-flow.app/Contents/MacOS/flow-engine`. The build copies and signs the supplied executable; use `--build-engine --jobs 2` to build the engine from this workspace before packaging instead. The engine version and input hash identify the dependency independently of the Flow source commit.

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

= Engine and preview development

Use the workspace Rust toolchain and Yarn dependencies for engine and preview work:

```sh
yarn install --immutable
node scripts/build.mjs build:preview
python3 scripts/flow-app.py build --build-engine --jobs 2
cargo fmt --check --all
cargo test -p typst-preview
yarn workspace typst-preview-frontend test run
```

The native engine build uses the locked `flow-release` profile with ThinLTO. The app manifest records its target, profile, Rust flags and any LTO override in addition to engine version and hashes. On Apple Silicon, use a native ARM Python environment.

= Documentation

Edit Typst sources, then regenerate the Markdown outputs. The generator uses an external `typlite` executable from PATH or `TYPLITE_BIN`. Prebuilt typlite executables are available from the #link("https://github.com/Myriad-Dreamin/tinymist/releases")[Tinymist releases]. Node.js supports this generator, and Node.js with Yarn is required for preview frontend and editor work.

```sh
node scripts/link-docs.mjs
node scripts/link-docs.mjs --check
typst compile --root . docs/tinymist/flow-app.typ dist/UserGuide.pdf
```

`README.md` and `docs/dev-guide.md` are generated. The app guide is bundled as a PDF during packaging. App sources compile independently of documentation conversion tools.

= Workflow

Use a short feature branch and Conventional Commits. Read the matching OpenSpec change before changing behavior and keep its artifacts aligned. Run the app tests and checks for the files touched. Engine and preview implementations are maintained alongside the app. Run targeted Rust crate tests and frontend tests for those changes, and keep installed engines independent of development build output.
