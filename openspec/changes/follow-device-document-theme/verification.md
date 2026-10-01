# Verification

## Implementation checks (2026-10-01)

- Frontend and DOM: 43 targeted tests passed (worktree baseline excluded), both TypeScript projects passed, preview production assets rebuilt.
- Rust: preview library 5 tests, standalone CLI theme-input test, and HTTP routing/origin 10 tests passed. Release engine built; all-target clippy for CLI, runtime, and preview library passed with warnings denied.
- Launcher: 8 configuration/recovery tests passed against a staged 0.1.2 bundle; its guide compiled successfully.
- `cargo fmt --check --all`, `git diff --check`, generated-doc check (with cached Yarn on PATH), and strict OpenSpec validation passed.
- `node tests/preview/theme-smoke.mjs`: independent variants, concurrent clients, live edits to both, correct source branch/revision for each palette, shared focus sequence, rejected untrusted origin and unknown theme path. The macOS temporary fixture uses its canonical path so filesystem events match watched dependencies.
- Browser: native light/dark palettes visibly rendered without inversion; System reacts to media changes; a Light override ignores subsequent device changes. ScrollTop 14000 and custom scale 1.5 survived switching; only one document instance remains. No browser errors reported.
- Browser touch-overlay test: a real DOM text range remained selected and the exact same SVG/WS route stayed active after device appearance changed; clearing the range applied the pending palette. Coarse-pointer mode was simulated for this check, not a physical iPhone/WebKit session.

## Deployment

Pending clean-commit bundle installation and existing URL verification. Physical iOS/iPadOS appearance and selection-handle checks remain user-side confirmation; do not treat browser emulation as that confirmation.
