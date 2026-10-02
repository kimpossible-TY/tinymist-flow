## 1. Define the Tailnet preview contract

- [x] 1.1 Record the exact-address, loopback-control-plane, fixed-port, and fail-closed requirements.
- [x] 1.2 Document why wildcard binding and relaxed Host-derived origin checks are out of scope.

## 2. Add the launcher

- [x] 2.1 Add a portable shell launcher that resolves the Tinymist binary and one MagicDNS hostname.
- [x] 2.2 Validate the configured port and address before starting the preview.
- [x] 2.3 Bind only the data plane to Tailnet, keep the control plane on loopback, disable local browser opening, and forward remaining preview arguments.

## 3. Document mobile use

- [x] 3.1 Add the Tailnet launch and mobile browser workflow to the source preview documentation.
- [x] 3.2 Document environment overrides, the no-wildcard rule, and the Tailnet ACL security boundary.

## 4. Validate

- [x] 4.1 Run shell syntax and fake-binary argument checks, including invalid input cases.
- [x] 4.2 Start a real preview on the Tailnet address and verify the page plus same-origin WebSocket access.
- [x] 4.3 Run the generated-document consistency check for the documentation change.


## 5. Tail Hosting HTTPS integration

- [x] 5.1 Add opt-in exact canonical HTTPS proxy Origin validation without changing direct HTTP defaults.
- [x] 5.2 Run preview Origin unit tests and build the release CLI.
- [x] 5.3 Verify real HTTPS document delivery and background LaunchAgent access for PDE.

## 6. Bounded continuous scrolling

- [x] 6.1 Bound partial SVG requests using viewport and page metadata with neighboring pages.
- [x] 6.2 Keep offscreen dummy pages as placeholders without canvas allocation.
- [x] 6.3 Coalesce viewport work and clean up pending timers on disposal.
- [x] 6.4 Build and deploy the frontend; restore PDE document mode.
- [x] 6.5 Confirm scrolling stability on the user's iPhone.

## 7. Follow changed locations

- [x] 7.1 Detect visual page changes and map the edited source text to the new document when possible.
- [x] 7.2 Carry the changed location before the ordered document delta, then navigate after the frontend finishes rendering.
- [x] 7.3 Jump directly across virtual pages and defer navigation during an active scroll gesture.
- [x] 7.4 Build and deploy the changed-location preview; confirm HTTP, WebSocket, and the 287-page document render.
- [x] 7.5 Confirm automatic change navigation on the user's iPhone.

## 8. Share an explicit preview selection with Codex

- [x] 8.1 Add an opt-in bounded local focus record with source snapshot, timestamp, viewer identity, and revision validation.
- [x] 8.2 Acknowledge persistence in the originating browser; keep taps independent of document rendering.
- [x] 8.3 Add Tail Hosting configuration, a focus reader, and workspace guidance for consuming selection context.
- [x] 8.4 Validate persistence, stale/unmapped handling, browser feedback, and live service deployment.
- [x] 8.5 Confirm a real iPhone tap followed by a Codex request with the user.

## 9. Native mobile text copying

- [x] 9.1 Exclude touch compatibility events and nested text-layer elements from mouse panning; explicitly allow native text selection/callouts.
- [x] 9.2 Build and deploy the updated frontend.
- [x] 9.3 Confirm long-press, selection handles, and Copy on the user's iPhone.
- [x] 9.4 Restore the managed PDE LaunchAgent after macOS Documents access is refreshed for the rebuilt executable.

## 10. Selection stability regression

- [x] 10.1 Defer document and viewport rendering while a native touch-overlay range is selected, then resume queued updates on collapse.
- [x] 10.2 Reject selection, long-press, drag, canceled and multitouch gestures as source taps.
- [x] 10.3 Handle visual viewport changes and keep focus feedback within the zoomed viewport.
- [x] 10.4 Add regression tests, build frontend, and validate the OpenSpec change.
- [x] 10.5 Verify document delivery from the updated installed preview.
- [ ] 10.6 Repeat the recorded native selection gesture on iPhone; do not claim WebKit painting is fixed from unit tests.

## 11. Restore the last edited location

- [x] 11.1 Add project/variant-scoped atomic last-edit persistence in the compilation watcher, including disconnected viewers and safe restart handling; add Rust tests.
- [x] 11.2 Send full-render resume hints and apply them after render without overriding theme-transition reading state; add frontend tests.
- [x] 11.3 Enable managed profiles, document persistence separately from assistant focus, and verify launcher behavior.
- [ ] 11.4 Build and verify edit/refresh/new-viewer/disconnected/restart behavior on temporary fixtures, then deploy and verify the existing PDE URL; record any renewed OS consent or physical-device checks.

2026-09-29 regression follow-up: 32 frontend/DOM tests, both TypeScript checks, preview and release-engine builds, 8 app tests, generated-doc consistency, and strict OpenSpec validation passed. A clean-source 0.1.1 candidate is retained at `dist/mobile-selection/tinymist-flow.app`. Deployment first exposed an asynchronous stop/start race (the stopped job remained briefly visible), then an explicit TCC Documents request for the new engine: existing code requirement mismatch and AUTHREQ_PROMPTING. Do not bypass consent. Restore 0.1.0 while waiting for the user to approve a coordinated reinstall. Mobile gesture verification and final release tagging remain pending.

Prior handoff state: the installed bundle was restored to 0.1.0 and its LaunchAgent restarted (PID 36841), but it did not open its HTTP listener after the outstanding consent request; HTTPS returned 502. Bundle rollback was confirmed, service recovery was not yet confirmed.

2026-09-29 consent follow-up: user approval restored HTTP 200 for 0.1.0. After confirming the old job fully stopped, installed the unchanged clean-source 0.1.1 candidate and started PID 37634. TCC logged another Documents code-requirement mismatch and AUTHREQ_PROMPTING at 20:19:37 KST. The updated bundle was left installed while awaiting its own approval (HTTPS 502 at that time).

2026-09-29 deployment verification: after the user approved the 0.1.1 Documents request, the managed PDE service at PID 37634 returned HTTPS 200 and delivered a complete 16,233,896-byte `new` document frame through a certificate-validated WebSocket. The served frontend contained the new selection and tap guards. Task 10.5 is complete. Task 10.6 remains pending a fresh iPhone selection-handle test; do not tag a confirmed release based on the transport check alone.

2026-09-28 device report: The user confirmed scrolling stability, following changed locations, and a tap followed by a Codex request on iPhone. Native selection inside SVG `foreignObject` was displaced. An HTML overlay outside SVG aligned the initial selection and enabled Copy, but a downward handle drag occasionally selected all preceding content. Keeping the overlay hit-testable between text rows, disabling SVG selection, and retaining the overlay DOM while selected resolved that behavior in repeated iPhone tests on HTTPS port 23632. Copy and short-tap location selection also worked there.

The same frontend binary now serves the production HTTPS port 23625. TypeScript/Vite and release CLI builds passed; HTTPS returned 200 and a WebSocket delivered the complete document frame. The user confirmed long-press, downward handle adjustment, Copy, and short-tap location selection in iPhone Safari on that address. After macOS administrator authentication, the Documents permission was switched back on and the temporary restart loop was stopped. The managed PDE LaunchAgent resumed on port 23625 with the original project and fonts paths. A certificate-validated WebSocket delivered a complete 16,630,540-byte document frame, and Tail Hosting's focus reader reported `service_matches: true` for the LaunchAgent PID. Restart reset the focus record to waiting; the next user tap establishes a fresh selection.
