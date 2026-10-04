# Last edited location restoration — 2026-10-02

## Change navigation regression — 2026-10-04

- The user reports the change button returning to the document's beginning across edits. The installed 0.1.4 engine reproduced two independent cases on an isolated 130-page fixture: changing a page-120 heading and replacing its include with a new file both emitted page 1 (the outline), rather than page 120.
- Edited-source mapping now uses bounded source ranges and newly introduced dependencies, traversing glyphs, graphics, and tagged body elements. It prefers actual elements over copied headers, then unlinked content over internal-link copies, retaining all ranks and candidates from all edited files. Generic editor source navigation is unchanged.
- Three scoped Rust tests passed: insertion/deletion/Unicode ranges including math identifiers, translated nested link boundaries, and external-link handling. Formatting, patch whitespace, strict OpenSpec validation, and scoped Clippy with warnings denied passed.
- Clean-source 0.1.5 was built from `84bf4fa34f373a338e22e1672ae5ccaea55c621c` with the native ARM ThinLTO profile. Bundled signed engine SHA-256: `20129d53dfb661cf9055f85a9af7f1084cc29ed7198e0c4daff4b0187a7e9fc5`; bundle verification passed.
- `change-smoke.mjs` passed against that final bundled engine: prior baseline/live-edit/nonvisual/reconnect/restart/project-isolation checks, plus both-theme new includes, linked outlines, headings copied into every running header, ordinary paragraphs, math identifiers, figure arguments, authored internal links, and persisted body-position restoration. Paragraph/math/figure assertions require nonzero source coordinates, excluding accidental page-only fallback.
- A real browser loaded at page 120. Native scrolling during a scheduled fixture edit deferred navigation and exposed the change button. Clicking it changed scrollTop from 176904 to 179712, moved the page-120 rectangle from y=2785 to y=-23, and hid the button. Reload restored the same location. Screenshot: `.local/jump-fix-20261004/button-jump.jpg`. No application errors were reported; an existing WASM initialization deprecation warning remains. Temporary fixture, server, and dependency symlink were removed after verification. Desktop checks do not complete the separate physical iPhone selection-handle task 10.6.
- Installed 0.1.5 at `~/Applications/tinymist-flow.app`, retaining the 0.1.4 backup at `~/Library/Application Support/tinymist-flow/releases/20261004-130916-482848/tinymist-flow.app`. The known-invalid production light/dark coordinates `[2,0,0]` were backed up to `.local/jump-fix-20261004/previous-change-record.json` and cleared while the old service was stopped; page fingerprints were retained. This does not invent a historical edit: the next visual edit records the corrected source target.
- An initial start overlapped the installer completing and reached the moved old bundle. Stopped that process (PID 19148), confirmed its exit, and restarted only the previously running PDE profile after replacement completed. Updated PID 19361 is attributed to the installed app/engine path. Ewald remains stopped and selected; Tail Hosting configuration and registered URLs are unchanged.
- At 13:10:17 KST, TCC logged a new Documents code-requirement mismatch and `AUTHREQ_PROMPTING` for the updated engine. System Settings showed Documents Folder on, but refreshing its existing permission opened an administrator authentication sheet. No credentials were entered. The user was asked to authenticate locally; do not bypass consent or cycle installed bundles. Actual production document delivery and final release tagging remain pending. Task 12.4 stays open.

### Permission applied and production delivery confirmed — 2026-10-04

- The user completed local authentication. System Settings then showed Documents Folder off; enabled it and selected Quit & Reopen. The resulting settings state confirms Documents Folder on. No credentials were entered by the agent, and the installed bundle was not rebuilt or replaced.
- Restarted the PDE engine to discard the access-denied source snapshot from before permission application. Both light and dark full documents arrived locally and through the existing certificate-validated HTTPS URL.
- The document had changed from 306 to 308 pages while the service was unavailable. Comparing the old fingerprints on restart recreated a page-2 fallback despite clearing its coordinates earlier. Backed up that known-invalid record to `.local/jump-fix-20261004/previous-change-record-after-consent.json` and reset its variants while the engine was stopped, establishing the current document as a fresh baseline without inventing an edit location.
- Final PDE engine PID 22196 serves installed 0.1.5. The final verification received four nonempty `new` frames: local light 17,663,404 bytes, local dark 17,716,796 bytes, HTTPS light 17,663,604 bytes, and HTTPS dark 17,717,308 bytes. Every frame carried a valid compiler revision; the palettes differ; no obsolete `resume` hint was sent. Evidence: `.local/jump-fix-20261004/live-delivery-final.json` and `.log`.
- Task 12.4 is complete. Ewald remains stopped; user document sources and the registered preview URLs were not changed. The next visual source edit supplies the corrected body position; readers should refresh once to clear any old client-side button target. The separate physical iPhone selection-handle task 10.6 remains pending.
- Confirmed release tag `flow-v0.1.5` points to the exact clean engine source commit `84bf4fa34f373a338e22e1672ae5ccaea55c621c`. No remote push or main-branch merge was performed.

## Implementation

- Clean source commit: `e298657d41b45d2bb1193b64161d4551ae808ea2`, on `feat/persist-preview-location`.
- Managed profiles configure `TINYMIST_PREVIEW_CHANGE_FILE` at Application Support `changes/<profile-id>.json`, independent of explicit assistant focus selections.
- Successful visual edits are persisted even without viewers. A connection's first full render can restore its variant's validated position; another viewer's full request does not navigate existing readers.
- Missing history establishes a baseline, not a guessed historical edit. Unchanged output survives service restarts; output changed while stopped falls back to the first changed page. Corrupt and different-project records are ignored.
- Theme transitions preserve captured reading position and zoom instead of applying the resume hint. Subsequent live edits still navigate normally, with recent-gesture deferral.

## Automated validation

- Preview Rust library: 9 tests passed, including persistence, disconnected edits, restart/stale output, variant separation, owner-only file permissions, and invalid history.
- CLI preview configuration: 1 test passed. HTTP tests with the `preview` feature enabled: 10 tests passed, including exact HTTPS origin validation and explicit theme routes. A preliminary HTTP command without that feature selected no tests and is not counted as validation.
- Frontend/DOM: 48 tests passed; both TypeScript projects and Prettier checks passed.
- App launcher/recovery: 8 tests passed against the final bundled app.
- `cargo clippy -p tinymist-preview -p tinymist-cli -p tinymist --all-targets -- -D warnings`, `cargo fmt --check --all`, and `git diff --check` passed.
- Preview assets, final release CLI, guide PDF, and the temporary 130-page Typst fixture built successfully. Generated-document consistency (`node scripts/link-docs.mjs --check`) and strict OpenSpec validation passed.
- `tests/preview/change-smoke.mjs` passed against the final engine: initial baseline, ordered light/dark edits, new viewer and existing-reader isolation, nonvisual edits, disconnected edits, restart, stale-output fallback, and project isolation.
- `tests/preview/theme-smoke.mjs` passed: concurrent native variants, live updates, variant-correct ordered focus, rejected origins, and unknown theme routes.
- The broad e2e/probe build was canceled; it is not reported as passed. Full `scripts/e2e.sh` was not run (`cargo-insta` is unavailable). The scoped preview protocol smoke tests exercise the requested workflow directly.

## Browser verification

- An isolated temporary preview visibly opened at page 120. After scrolling away, reload returned to page 120; a second browser did the same without moving the first reader (scrollTop 14000, scale 1.25).
- A System light-to-dark transition retained scrollTop 14000 and scale 1.25, with one renderer and an empty patch queue. A later live edit navigated to page 120. Dark-mode reload also restored page 120.
- A simulated recent wheel gesture deferred a live edit, kept scrollTop 14000, and displayed Jump to change. Clicking the button navigated to page 120.
- No browser application errors or error overlays were observed. Both isolated browser sessions and the temporary server/project were closed and removed.
- These desktop checks do not constitute physical iPhone/iPad Safari or native selection-handle confirmation. Existing task 10.6 remains pending.

## Installed candidate and consent blocker

- Installed `~/Applications/tinymist-flow.app` version **0.1.3**, built from the clean source commit above (`sourceDirty: false`). Bundled signed engine SHA-256: `90d63a7547a87e1ab0c65e877b8327259afb0af7ba8cf505944446404e56a916`.
- Before replacing the app, the prior PDE job was stopped and PID 71315 plus its port-23625 listener were confirmed gone. The standard installer retained the prior 0.1.2 bundle at `~/Library/Application Support/tinymist-flow/releases/20261002-220818-799961/tinymist-flow.app`.
- Started only the previously running PDE profile and opened the updated app. Ewald remains stopped. The existing project, fonts, ingress port, HTTPS origin restrictions, and URL are unchanged.
- New PDE engine PID 79096 is waiting before HTTP bind. At **22:08:29 KST**, TCC logged an existing-code-requirement mismatch and `AUTHREQ_PROMPTING` for `kTCCServiceSystemPolicyDocumentsFolder` attributed to `io.github.kimpossible-ty.tinymist-flow`.
- At verification time the loopback listener and new PDE change record do not yet exist, and `https://kimtaeyoungs-macbook-air-daemon.tail8adc61.ts.net:23625/` returns **502**. Running process status is not readiness confirmation.
- Leave the correct updated bundle installed, without a rollback/reinstallation cycle. Task 11.4 remains pending renewed user Documents consent and actual PDE URL/document delivery verification. No confirmed release tag, main merge, or push was performed.

## Computer Use follow-up — 2026-10-03

- The user explicitly requested direct permission approval and service start. System Settings showed tinymist-flow's Documents Folder switch on, but TCC still logged a code-requirement mismatch and `AUTHREQ_PROMPTING`. The old PID 79096 served HTTP 200 without a rendered document or change record; this was not successful recovery.
- Stopped the old PDE job, confirmed its exit, and started the existing installed app again as PID 79550. A process sample shows startup waiting while opening the configured fonts directory. The HTTPS endpoint currently returns 502.
- Attempting to refresh Documents access in System Settings opened an administrator Password/Unlock sheet. No password was entered and the permission change has not completed. The Computer Use tool also explicitly disallowed access to the UserNotificationCenter app, so its pending notification could not be operated directly.
- The System Settings authentication sheet remains open for the user's local authentication. Continue with the Documents switch and PDE delivery verification after it is unlocked; do not rebuild or replace the app.

## Permission applied and PDE verified — 2026-10-03

- After the user completed administrator authentication, System Settings showed the Documents Folder switch off. Enabled it for tinymist-flow and selected Quit & Reopen in the macOS prompt. The resulting settings view confirms Documents Folder on.
- The already-started PDE engine PID 79550 resumed. The existing HTTPS URL returns HTTP 200. The installed 0.1.3 app and engine were not rebuilt or replaced.
- The managed `changes/typst-pde.json` record now exists with mode 0600. Both light and dark variants have 307 successful page fingerprints. Their positions are null because this first successful compilation establishes the baseline; the next visual edit will supply the saved location.
- A fresh Chrome tab on the production URL rendered the actual PDE cover, including its title, author, equation, and document links. This confirms document delivery beyond HTTP availability. The temporary verification tab is left to normal turn cleanup.
- Task 11.4 is complete. The physical iPhone selection-handle check in task 10.6 remains pending; no claim is made that desktop verification completes that check. No release tag, main merge, or push was performed.
