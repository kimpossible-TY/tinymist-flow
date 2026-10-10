# Verification and deployment — 2026-09-28

## Verified

- Swift 5 compilation with warnings treated as errors.
- Eight targeted configuration and update-recovery tests passed.
- Six Tail Hosting management/focus tests passed against a temporary patched copy.
- A real signed app bundle was installed, updated, and rolled back in an isolated temporary directory.
- Strict bundle signature, engine SHA-256, app version, and engine execution checks passed.
- Native UI opened with the branded icon and imported project controls; login startup was enabled.
- Installed version: 0.1.0, source revision `47149701d1ee9d79c5a0e189425f2faf8383e4b9`, clean build, ad-hoc signing.
- OpenSpec strict validation passed. Generated README consistency passed; the existing book template reports its unavailable `Source Han Serif SC` font on this host.

## macOS consent and live verification

The migration adapter was applied and the GUI Restart action launched the bundled engine under the existing preview job label. macOS TCC attributed the Documents request to `io.github.kimpossible-ty.tinymist-flow`, confirming the new identity. Computer Use refused access to the system UserNotificationCenter app for safety reasons, so the user completed Documents consent.

During the wait for consent, the previous Tail Hosting registry, launcher, and LaunchAgent were restored from the migration backup. The legacy preview resumed after consent. The migration was then reapplied and the imported profile restarted. The app and launchd both reported PID `63386` and `managedByApp: true`. The local HTTP endpoint returned 200. The unchanged Tailscale Serve route proxied the HTTPS URL to `127.0.0.1:23625`; the WebSocket returned 101 and delivered a complete 16,273,700-byte document frame.

System Settings showed `tinymist-flow` → `Documents Folder: on`. The app window showed `프리뷰 준비됨 · PID 63386` and login startup enabled. A Safari tap on the cover title recorded source `cover.typ` line 9 in the new focus file. Tail Hosting reported `service_matches: true`, `source_excerpt_matches_disk: true`, and `needs_reselection: false`; its focus PID was also `63386`. Six Tail Hosting management/focus tests passed after migration. No source document was moved and no Tailscale route was changed.

Unattended login after a real macOS login cycle and iPhone behavior after this packaging migration were not rechecked. The preview engine's existing iPhone implementation was not changed here. Ad-hoc signing caused a second Documents prompt when the final bundle replaced the earlier trial bundle. Future releases should use a stable code-signing identity and verify permission continuity.

## Repository naming — 2026-10-04

The development fork was renamed from `typst-relay` to `tinymist-flow`, preserving GitHub repository ID `1376877083` and its `Myriad-Dreamin/tinymist` parent. The earlier independent repository became `tinymist-flow-archive`, preserving ID `1376906710`. The repository description, local remote URLs, current documentation, and README source now use the product name. `git ls-remote` confirmed access to both repositories and their original branch heads.

The local typlite build, JavaScript syntax check, README regeneration, OpenSpec strict validation, and diff whitespace check passed. Both Typst sources compiled to PDF; the README also compiled to HTML, where the current repository, issues, and historical repository links were verified. The README template still reports the existing unavailable `Source Han Serif SC` and `BlexMono Nerd Font Mono` fonts; HTML export also reports its experimental-feature warning.

## Flow-only source ownership — 2026-10-04

Commit `9dfeb20f0193b0ace1e1be5ac082cc0f20dbfe9c` removes 3,033 unrelated tracked files and retains a 32-file app project. Swift app sources, branding, the Tail Hosting adapter, packaging, app tests, standalone documentation, Flow OpenSpec artifacts, workflow skills, and Apache License 2.0 remain. Rust engine crates, editor integrations, preview frontend sources, engine fixtures, dependency workspaces, and upstream release/CI files are no longer maintained in this tree. Prior sources remain in Git history. Previously generated local engine and editor outputs were preserved under `.local/retired-engine-artifacts/20261004-201615/`.

Packaging consumes an external executable via `--engine`, `FLOW_ENGINE_PATH`, or the current installed app. It probes required preview CLI support and completes and verifies a staged bundle before replacing its build output. The release manifest records the external engine version and input SHA-256 separately from the Flow source revision and bundled engine SHA-256. Native runtime preview arguments and focus/change/origin environment variables are preserved. The repository description now identifies Flow as a native macOS app.

Verification passed both in the development tree and a clean shallow checkout of the app-only commit:

- Fourteen tests passed, including fresh Swift compilation with warnings treated as errors, profile validation, missing/incompatible engine rejection, failed-build preservation, update recovery, real bundle signing, temporary installation, and HTTP/WebSocket preview delivery.
- Real preview checks confirmed system-theme support and distinct compiled light/dark documents through the configured HTTPS origin.
- The external engine was supplied from `~/Applications/tinymist-flow.app/Contents/MacOS/flow-engine`; its input hash remained unchanged. The fresh checkout contained no engine workspace, Cargo manifest, or target directory and stayed clean after the checks.
- The README and development guide were regenerated with an external typlite executable and passed `node scripts/link-docs.mjs --check`. JavaScript syntax and diff whitespace checks passed.
- All three standalone Typst documents compiled to PDF without warnings, and OpenSpec strict validation passed.
- The source commit was integrated into local main by fast-forward.

This extraction does not install or restart the live app. Existing mobile selection, assistant focus, device theme, and change restoration features continue to depend on the compatible external engine. A replacement engine requires compatibility verification; the CLI probe alone cannot establish support for every environment contract. CI compiles current Swift sources and the documentation; real-engine integration remains opt-in through `FLOW_TEST_ENGINE`.

## Preview branch integration — 2026-10-08

The preview activity status branch is integrated with the engine, preview frontend and supporting workspace sources restored at the user’s request. The merge retains the tinymist-flow repository identity, external engine provenance, staged packaging and freshly compiled Swift app tests. Native ThinLTO engine builds and deferred consent verification are also retained. Historical app-only extraction records above describe the earlier layout.

- App unit tests: 14 passed; 2 optional bundle tests skipped in the unit run.
- Optional external-engine bundle tests: both passed, including signature/provenance, isolated installation and both theme variants through the configured WebSocket origin. The installed engine input hash was preserved.
- Preview frontend: all 40 tests passed.
- Restored workspace: locked offline Cargo metadata and cargo fmt --check --all passed.
- User guide: compiled successfully with the compatible installed engine.
- The restored typlite binary built successfully offline; README and developer guide were regenerated and scripts/link-docs.mjs --check passed.
- OpenSpec CLI was unavailable locally; artifact presence and requirement/scenario structure were checked manually.
- Restored engine, vendor, frontend and preview/performance fixtures match the incoming branch exactly; the merge resolution diff against the incoming branch passed git diff --check. The complete staged restoration retains pre-existing whitespace findings in upstream files and fixtures; those sources were kept unchanged.

No live app installation, service migration or release tag was performed during integration.

## Persistent personal signing — 2026-10-10

Packaging can save an existing Keychain identity with `configure-signing --identity`, then reuse its certificate fingerprint for both app and engine builds. Explicit CLI and environment overrides retain precedence. Missing or malformed saved identities fail without replacing the previous output; configuration is saved only after a signing probe succeeds. Bundle validation checks the recorded public certificate SHA-256 against both signatures before executing either binary.

- Twenty app/configuration/recovery tests passed, including fresh Swift compilation with warnings treated as errors.
- Both external-engine integration tests passed: bundle provenance/signature and isolated installation, plus distinct light/dark document delivery through the configured WebSocket origin. The optional persistent-certificate integration test remains pending initial Keychain approval.
- The user guide compiled successfully, OpenSpec strict validation passed, and the patch passed whitespace checks.
- A personal code-signing identity named `tinymist-flow Local Signing` was created in the user's login Keychain. Its public certificate SHA-1 is `48CCFB52FB49CBE01B5FF5A0FD5859AFA8ADDBF1`. The private key remains in Keychain; temporary key/PKCS#12 files were removed. No general certificate trust or TCC database changes were applied.

The first signing probe opened native Keychain authorization. Computer Use refused access to `com.apple.SecurityAgent` for safety reasons; the user reported being unable to access the Mac. The pending probe was cancelled, and `signing.json` was not written. The installed app was not replaced during this signing work. Registration, certificate-signed build checks, initial Documents consent, live HTTPS delivery, and permission continuity after a subsequent installed build remain unverified. Resume by rerunning `python3 scripts/flow-app.py configure-signing --identity "tinymist-flow Local Signing"` and completing the native key-use authorization locally.

The user subsequently completed native Documents and Keychain Always Allow authorization. A first real signing check exposed an optional-argument parsing issue in certificate extraction; using `--extract-certificates=PREFIX` fixes it. Identity registration then succeeded, and all 23 tests passed with the real certificate, including distinct app builds satisfying the same designated requirements for both app and engine. Repeated signing completed without another Keychain prompt. Restarting the existing ad-hoc app restored live HTTPS document delivery for both light and dark themes. Certificate-signed installation and update continuity checks are the remaining deployment work.
