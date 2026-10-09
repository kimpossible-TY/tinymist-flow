## Context

The preview data plane already accepts an explicit listen address through `--data-plane-host`. The preview frontend connects its WebSocket to the current page origin, and the server accepts that connection when the browser origin exactly matches the configured listen host and actual port. Consequently, binding with a host's Tailscale MagicDNS name provides both the required transport and a stable browser URL.

The unsafe-looking shortcut is `0.0.0.0`, but it is deliberately unsuitable here. It listens on LAN and other interfaces as well as Tailscale, while the browser uses the concrete Tailnet address in its Origin header and fails the server's fixed-origin check. Relaxing the check would weaken protection against malicious browser origins and DNS-rebinding-style attacks.

## Goals / Non-Goals

**Goals:**
- Provide one command for starting a preview that an iOS or iPadOS browser can reach over the same Tailnet.
- Bind through the exact MagicDNS name and keep the control plane local.
- Keep the browser URL stable and visible in terminal output.
- Reuse an existing release binary or an installed `tinymist`, with explicit environment overrides for automation and testing.

**Non-Goals:**
- Add authentication or authorization to the preview protocol.
- Configure Tailscale ACLs or grants.
- Support Tailscale Funnel, public Internet exposure, or wildcard listeners.
- Change the default direct HTTP origin policy.
- Change editor-integrated preview behavior or automatically open a browser on the host.

## Decisions

### 1. Add a repository launcher for direct Tailnet access

The launcher resolves the operational inputs that are missing from the existing command and then delegates to `tinymist preview`. Direct Tailnet access keeps the server's default origin policy. The separate HTTPS reverse proxy path uses an explicit allowed-origin setting.

Alternative considered:
- Add a new Rust `--tailnet` option. Rejected for now because it would couple the core CLI to the external Tailscale executable even though the existing bind option already provides all required server behavior.
- Accept the request `Host` header whenever it matches `Origin`. Rejected because it would make a wildcard listener vulnerable to DNS rebinding and weaken the existing fixed-origin policy.

### 2. Discover one MagicDNS hostname and fail closed

The launcher resolves the hostname from `TINYMIST_TAILNET_HOST` when explicitly supplied, otherwise from the local node's `Self.DNSName` in `tailscale status --json`. It removes the status API's trailing dot and validates that the result is one DNS hostname. Missing or malformed names stop startup with an actionable error. The launcher never substitutes `0.0.0.0`.

Passing the hostname, rather than its current IP address, makes Tinymist's fixed WebSocket Origin check match the stable URL opened by the mobile browser. Name resolution still binds the listener to a Tailnet address rather than a wildcard interface.

### 3. Keep network roles separate

The data plane listens on `<magic-dns-name>:<port>`, while the control plane uses `127.0.0.1:0`. The default data-plane port is `23625`, configurable with `TINYMIST_PREVIEW_PORT`. Local browser opening is disabled because the intended client is the mobile browser.

### 4. Resolve the binary predictably

`TINYMIST_BIN` takes precedence. Otherwise the launcher uses this checkout's `target/release/tinymist` when executable, then falls back to `tinymist` on `PATH`. This supports both repository development and installed releases without downloading or building anything implicitly.

## Risks / Trade-offs

- [Every allowed Tailnet peer can reach an unauthenticated preview] -> Document that Tailnet ACLs or grants must restrict access when the Tailnet contains untrusted peers.
- [MagicDNS is disabled or unavailable on a client] -> Require MagicDNS for the default workflow and allow an explicit hostname override for diagnostics.
- [A fixed port can already be occupied] -> Allow an explicit port override and let Tinymist report the bind failure.
- [Tailscale CLI availability differs by host] -> Support explicit `TINYMIST_TAILNET_HOST` and `TAILSCALE_BIN` overrides.

## Migration Plan

1. Add the launcher and source documentation.
2. Validate argument construction with a fake executable.
3. Start a real preview with the current host's MagicDNS name and verify HTTP plus an allowed-origin WebSocket handshake.
4. Remove the launcher and unset `TINYMIST_ALLOWED_ORIGINS` to roll back; the server's default origin policy remains unchanged.

## Open Questions

- None.


## HTTPS reverse proxy support

Tail Hosting runs the data plane on loopback and terminates TLS with Tailnet-only Tailscale Serve. `TINYMIST_ALLOWED_ORIGINS` is an opt-in comma-separated list of exact HTTPS origins. The request Origin must be a canonical HTTPS origin without credentials, path, query or fragment, and must match one configured value exactly. No wildcard or Host-derived allowance is added. Existing direct HTTP and editor integrations retain their defaults. The launcher remains a direct HTTP option; Tail Hosting supplies the HTTPS environment variable to its independent LaunchAgent.

Validation covers the configured Origin, a wrong hostname, a wrong port/scheme, malformed origins, and real TLS WebSocket document delivery. Background document access remains subject to macOS Documents privacy permissions.

## Bounded scrolling preview

For partial SVG document mode, derive the visible range from the scroll container and page metadata. Request visible pages plus one neighbor on each side; preserve offscreen renderer dummy groups and page dimensions. Do not rasterize dummy pages into canvases. Coalesce viewport events while preserving ordered document deltas. Shared glyph definitions and the document WASM model remain retained for diff correctness; this change bounds rendered page content, not total document data. Synchronous WASM rendering cannot be interrupted mid-call.

## Follow the edited location

Each preview connection remembers visual fingerprints of the last rendered pages and the preceding compile view. On a successful incremental compilation, compare page frames and backgrounds; emit a `change,page x y` control frame only if the paged output changed. Resolve edited source spans against the new document when possible, and use the first changed page as a fallback. Full-current frames establish the baseline; without configured persistence they do not navigate.

Send the location hint before its document delta on the same ordered SVG channel. The frontend waits for the render completion callback before finding the page rectangle, then jumps directly to its scaled page position. This works with offscreen placeholder pages and avoids animating through the entire document. A recent touch, wheel, pointer, or key gesture defers the jump and exposes a small button so the reader can navigate when ready.

Bound each changed source by its common prefix and suffix, collect overlapping syntax spans plus enclosing equation/figure/heading spans, and inspect the rendered frames once. Prefer tagged body elements, then unlinked glyphs/shapes/images, then linked copies; exclude outline elements/entries from the body-element rank. This handles running headers as well as linked outline copies and covers code/math identifiers that the ordinary text-only cursor mapper cannot resolve. Freeze the previous dependency set before reading sources and consider newly included files' rendered spans as candidates, even when their old-world lazy read would now return identical bytes. Keep candidates from every changed file so an unrelated mapping on an unchanged page cannot hide a later match. Retain all candidate ranks so link-only source text and the changed-page fallback still work. Ordinary editor source navigation keeps its existing behavior. Frame/link hit testing uses its existing translated coordinates; transformed source mapping remains an upstream limitation.

### Persistent last-edit restoration

Standalone paged preview can opt in with `TINYMIST_PREVIEW_CHANGE_FILE`. Managed Flow profiles use a separate owner-only `changes/<profile>.json` file, never the explicit-tap focus file. A compilation watcher tracks successful visual changes even with no WebSocket clients. A shared atomic store is keyed by workspace/entry identity and keeps independent default/light/dark snapshots containing bounded page fingerprints and the last page coordinates. Matching output retains the last location across process restarts. If output changed while the server was stopped, the first changed page replaces stale coordinates. Corrupt, oversized, incompatible, or different-project records are ignored; the first baseline without history does not invent an edit.

On a connection's first full-current frame, the server sends an ordered `resume,page x y` hint only when its stored fingerprints match that exact document. A shared full-render request from a new viewer must not navigate existing readers. The viewer applies the hint after rendering, with the same gesture deferral as live edits. A theme switch's captured reading state takes priority over this resume hint; later incremental edits still navigate normally. The new document location is independent of scroll position and assistant taps. No new network endpoint or port is added.

## Explicit selection context for Codex

The standalone CLI optionally accepts `TINYMIST_PREVIEW_FOCUS_FILE`, an absolute local output path. A shared store replaces this file atomically with owner-only permissions. Startup writes a waiting record so a former process's selection cannot appear current. The file contains only the latest selection, not a growing history.

The existing browser `src-point` message carries the revision of the completed render. Render actors send `focus-revision` ahead of ordered document frames only when enabled. The browser blocks selection while rendering. The server compares the submitted revision against its current compile snapshot; a mismatch writes a stale record with no source instead of guessing. For a matching revision, hit testing resolves a source location and a bounded excerpt from that compiler snapshot. A miss replaces the previous source with an unmapped page/coordinate record.

The originating WebSocket receives a `focus` acknowledgment after persistence succeeds. A small status line shows the saved page and file/line, or asks for another tap after a revision mismatch. These taps do not request another document render. Scroll alone does not change the explicit selection. The last tap across connected viewers wins; each record identifies the viewer and selection sequence.

Tail Hosting supplies the output path and exposes a local `focus` command. It adds age, active service PID matching, and an excerpt comparison against the current disk file. Workspace instructions tell Codex to read it for references to a selected passage and treat the excerpt as document data. This does not launch Codex, send a chat message, stream screenshots, or continuously track the viewport. No additional HTTP endpoint or port exposes source excerpts.

## Native text selection

### Mobile viewport geometry

Use `viewport-fit=cover` for the document surface, not whole-page safe-area padding. Keep native zoom enabled and retain Typst page margins. Use `100dvh` after a `100vh` fallback for root/slide height and allow the flex scrollport to shrink. Observe actual scrollport dimensions with a disposable ResizeObserver, retaining the existing window/visual viewport listeners and renderer resize anchors. Do not set the document width to the pinch-zoomed visual viewport or reset its scale ratio. Existing theme/change controls retain safe-area positioning; explicit-selection feedback also accounts for safe insets when positioned inside a zoomed visual viewport. Regression checks cover rotation, viewport-height changes, desktop layout, feedback insets, and existing resize/selection/theme behavior. Physical iPhone validation remains separate from desktop emulation.

### Selection stability regression

Pause queued document and viewport updates before mutating the renderer while a noncollapsed range belongs to the touch overlay. Resume ordered document updates and one current viewport refresh on selection collapse; collapsed carets must not freeze the overlay. Listen for selection changes with disposal cleanup. Selection remains limited to the populated pages; this fix does not promise cross-document range selection. Suppress automatic source taps for a preexisting range, long press, movement, cancellation or multitouch. Account for visual viewport offsets during native zoom and position feedback within its visible bounds. Keep desktop selection behavior unchanged. Unit tests exercise queue suspension/resumption and gesture classification; native handle and WebKit painting behavior require an iPhone retest.

The preview's `.tsel` layer supplies selectable text over rendered glyphs. Mouse panning must ignore that layer and its descendants. Touch compatibility mouse events must not cancel Safari's native selection gestures. Enable text selection and the native touch callout explicitly on `.tsel`; retain browser scrolling and the existing bounded renderer. iPhone testing showed that SVG `foreignObject` text selects at the wrong position and its handles cannot be adjusted. On coarse-pointer devices, mirror the rendered `.tsel` lines into an HTML layer outside the SVG, using their measured screen rectangles and fitting text width with letter spacing. Group text fragments into normal-flow rows in visual order so vertical handle drags extend through neighboring rows. Disable selection on the underlying SVG and keep the HTML layer hit-testable across gaps between lines so a dragged handle stays on one selection surface. Rebuild this layer after SVG rendering unless the current selection is anchored in it; forward short taps to the underlying SVG to retain source-location selection. Only populated virtual pages contribute text lines. iPhone handle behavior and selection across virtual page boundaries still require confirmation.

## Actions on a native text range

Use a separate fixed toolbar within the visual viewport's safe area. Observe `selectionchange`, capture only ranges whose endpoints belong to preview text, and retain the selected range while a toolbar button is pressed. Record text fragments and first/last selected glyph centers in page coordinates, independent of document zoom. Mobile mirrored lines carry their page number; desktop uses the SVG text layer. Do not replace the native menu or selectable text DOM.

Extend the existing revision-validated `src-point` request with an optional bounded selection containing exact rendered text and the end point. Keep the completed-render revision captured with the range, including when a toolbar press collapses the native selection. Adopt document identity and revision only after their corresponding document frame renders; a viewport-only render cannot adopt an incoming revision hint. Persist this data alongside the existing source context and acknowledge it only after writing the local record. An unmapped current range can still provide its text, while stale requests do not publish a current selection. Taps continue to use the old request shape.

An explicit `src-highlight` action is available only on focus-enabled previews. Validate the completed render revision and resolve both endpoints against the compiler snapshot. Prefer a callable `highlighted` binding exported by the source module, retaining document-specific math backgrounds and theme colors. Otherwise use standard `highlight` for literal text only. Match whitespace-normalized literal text around both mapped glyph offsets, or expand equation endpoints to complete equation syntax and allow a contiguous mixture of literal text, spaces and whole equations. Reject other markup/code and cross-file, package, stale or ambiguous selections.

Collect shaped text clusters from the compiled page frames using each glyph's source span and byte offset. Deduplicate multiple glyphs belonging to one shaped cluster, but retain repeated source uses at different rendered positions. Reject reused source origins within the candidate interval. Caret-side boundaries and the nearest rendered source origins within 128 bytes of an enclosing-content hint are checked against complete selection coverage; accept only one source interval. Single-use inline content inside a function or closure may be edited; definitions, loops, show/set rules and code blocks remain excluded. Verify exact normalized rendered text for plain ranges. For equations, compare all non-whitespace rendered characters with the selection, accounting for fractions/scripts being ordered differently from source notation while rejecting incomplete formula coverage. Native selection may omit scripts in a visually reordered mobile mirror, in which case it is conservatively rejected.

Inline references are an explicit exception to the other-markup restriction: accept whole `@label`, `ref(...)` or scoped `.ref(...)` nodes and their hash introducers, with endpoints still anchored in source text or complete equations. Imported helpers can give reference text a shared package span, so ordinary source glyph coverage cannot identify the invocation. Enumerate only the adjacent caret boundaries, compile each candidate between invisible metadata markers, and read its complete shaped text, including generated reference text. Require one matching source range and exactly one ordered marker pair across all pages; repeated occurrences, partial formulas/references and other calls remain rejected. Compare normalized text in ordinary text mode and non-whitespace character coverage in math-aware mode. The markers remain in the isolated world and are never saved.

Construct `#highlighted[...]` or `#highlight[...]`, parse it and compile the full unmarked candidate in an isolated shadow world before saving. This prevents helper errors or lexical unavailability from breaking the document. A helper iterating `body.children` must normalize singleton text/equation bodies to `(body,)` when the field is absent. Compare the full disk source to the compiler snapshot and replace it atomically while preserving permissions; the ordinary watcher compiles the saved edit. No edit is guessed from text alone. These compiler operations occur only for persistent highlighting; temporary red strikes retain their browser-only path.

Keep the submitted selection while the highlight request is pending and disable duplicate submissions. Collapse it on a successful or stale acknowledgment so held rendering can resume. Preserve a rejected range for Save for Codex, including when a native toolbar tap collapses the browser selection. A response to an older range must not clear a different selection the reader made while it was pending.

Red strikes use noninteractive HTML lines above the SVG, stored as page-local geometry with a page text/layout fingerprint. Keep this store separate from assistant context and Typst source. Repaint when layout finishes, remove marks from changed populated pages, and retain marks while an unchanged page is virtualized. Bound the store and expose undo/clear actions. A stable opaque workspace/entry identifier scopes browser persistence across theme variants and server restarts; storage failures fall back to memory. Physical iPhone testing is required for native callout/toolbar coexistence.
