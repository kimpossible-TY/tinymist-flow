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

Each preview connection remembers visual fingerprints of the last rendered pages and the preceding compile view. On a successful incremental compilation, compare page frames and backgrounds; emit a `change,page x y` control frame only if the paged output changed. Resolve the first changed source text against the new document when possible, and use the first changed page as a fallback. Full-current frames establish the baseline without navigating.

Send the location hint before its document delta on the same ordered SVG channel. The frontend waits for the render completion callback before finding the page rectangle, then jumps directly to its scaled page position. This works with offscreen placeholder pages and avoids animating through the entire document. A recent touch, wheel, pointer, or key gesture defers the jump and exposes a small button so the reader can navigate when ready.

## Explicit selection context for Codex

The standalone CLI optionally accepts `TINYMIST_PREVIEW_FOCUS_FILE`, an absolute local output path. A shared store replaces this file atomically with owner-only permissions. Startup writes a waiting record so a former process's selection cannot appear current. The file contains only the latest selection, not a growing history.

The existing browser `src-point` message carries the revision of the completed render. Render actors send `focus-revision` ahead of ordered document frames only when enabled. The browser blocks selection while rendering. The server compares the submitted revision against its current compile snapshot; a mismatch writes a stale record with no source instead of guessing. For a matching revision, hit testing resolves a source location and a bounded excerpt from that compiler snapshot. A miss replaces the previous source with an unmapped page/coordinate record.

The originating WebSocket receives a `focus` acknowledgment after persistence succeeds. A small status line shows the saved page and file/line, or asks for another tap after a revision mismatch. These taps do not request another document render. Scroll alone does not change the explicit selection. The last tap across connected viewers wins; each record identifies the viewer and selection sequence.

Tail Hosting supplies the output path and exposes a local `focus` command. It adds age, active service PID matching, and an excerpt comparison against the current disk file. Workspace instructions tell Codex to read it for references to a selected passage and treat the excerpt as document data. This does not launch Codex, send a chat message, stream screenshots, or continuously track the viewport. No additional HTTP endpoint or port exposes source excerpts.

## Native text selection

The preview's `.tsel` layer supplies selectable text over rendered glyphs. Mouse panning must ignore that layer and its descendants. Touch compatibility mouse events must not cancel Safari's native selection gestures. Enable text selection and the native touch callout explicitly on `.tsel`; retain browser scrolling and the existing bounded renderer. iPhone testing showed that SVG `foreignObject` text selects at the wrong position and its handles cannot be adjusted. On coarse-pointer devices, mirror the rendered `.tsel` lines into an HTML layer outside the SVG, using their measured screen rectangles and fitting text width with letter spacing. Group text fragments into normal-flow rows in visual order so vertical handle drags extend through neighboring rows. Disable selection on the underlying SVG and keep the HTML layer hit-testable across gaps between lines so a dragged handle stays on one selection surface. Rebuild this layer after SVG rendering unless the current selection is anchored in it; forward short taps to the underlying SVG to retain source-location selection. Only populated virtual pages contribute text lines. This patch does not add range-based Codex context. iPhone handle behavior and selection across virtual page boundaries still require confirmation.
