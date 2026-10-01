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

- Installed 0.1.2 from clean implementation commit `d23fdee5`; engine SHA-256 `29b509504ac60d863a746eb343bd7e1f763499c384e61c2ca2f4d23e5240a613`.
- Previous bundle preserved at `~/Library/Application Support/tinymist-flow/releases/20261001-215312-644365/tinymist-flow.app`. PDE was stopped and confirmed absent before replacement, then restarted with `--follow-system-theme`; the menu app was relaunched. Ewald was already stopped and remains stopped.
- PDE engine PID 59245 is waiting before HTTP bind. At 21:53:14 KST, macOS TCC logged a mismatch with the previously approved app code requirement and `AUTHREQ_PROMPTING` for `kTCCServiceSystemPolicyDocumentsFolder`. Loopback HTTP has no listener and tailnet navigation fails while consent is pending. Process status alone does not establish readiness.
- The correct updated bundle remains installed, without an automatic rollback/replacement cycle. Existing URL delivery verification (task 3.3) is pending renewed user Documents approval; no confirmed release tag was created.
- Physical iOS/iPadOS appearance and selection-handle checks remain user-side confirmation; do not treat browser emulation as that confirmation.
