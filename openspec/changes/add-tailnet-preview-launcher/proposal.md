## Why

Tinymist's preview server can already listen on a specific non-loopback address, but using it safely from an iPhone or iPad currently requires discovering the host's Tailscale address and assembling several hidden preview arguments by hand. Binding to `0.0.0.0` is both broader than necessary and incompatible with the preview server's fixed-origin WebSocket check.

For a mobile review workflow, edits happen on the development host while the mobile device only needs a browser view. A small repository launcher can expose the existing preview data plane through the host's stable MagicDNS name without changing the preview protocol or weakening its origin checks.

## What Changes

- Add a `scripts/tailnet-preview.sh` launcher that discovers the host's MagicDNS name and starts `tinymist preview` on the address resolved from that name.
- Keep the control plane on loopback, disable local browser opening, and use a fixed configurable port so the mobile URL remains stable.
- Fail closed when a Tinymist binary, a usable MagicDNS name, or a valid port cannot be resolved; never fall back to a wildcard listener.
- Document the mobile Tailnet workflow, overrides, URL, and security boundary in the source preview documentation.

- Support loopback preview behind Tailscale Serve through an explicitly configured HTTPS Origin (`TINYMIST_ALLOWED_ORIGINS`).

## Capabilities

### New Capabilities

- `tailnet-preview-access`: start a live Typst preview on an exact Tailnet address for viewing from a mobile browser.

### Modified Capabilities

## Impact

- `scripts/tailnet-preview.sh`
- `docs/tinymist/feature/preview.typ`
- Shell-level launcher validation and a live HTTP/WebSocket smoke test against the host's Tailnet address
- Add opt-in exact HTTPS proxy Origin matching to the preview HTTP server; retain existing defaults and the WebSocket protocol.

- Bound continuous partial SVG rendering to the viewport and adjacent pages; keep offscreen placeholders free of canvas allocations and coalesce viewport work for mobile use.

## Explicit preview selection for Codex

Add an opt-in local focus file for mobile review. A document tap records its page, mapped source location, bounded source excerpt, time, and compiler revision; the preview acknowledges persistence. Codex reads this context when the user refers to the selected passage.

## Native text copying on mobile

Fix the recorded selection regression by synchronizing SVG and HTML selection lifetimes, rejecting non-tap gestures, and accounting for the mobile visual viewport. Device verification remains required; the recording alone does not establish a WebKit rendering root cause.

Preserve native touch text selection and copy menus. Restrict custom document dragging to a primary mouse button on fine, hover-capable devices, and exclude all descendants of the text-selection layer.

## Restore the last edited location

Persist the most recent visual edit per project and document variant on the server, including edits made without connected viewers. New and refreshed viewers resume there after rendering. Keep explicit assistant selections separate and preserve reading state during palette changes.

## Edge-to-edge mobile viewport

Remove the browser's landscape safe-area gutters from the document surface, retain safe positioning of controls/feedback, and follow dynamic viewport height and settled scrollport dimensions without resetting zoom or reading position. Implement this as an external engine update in an isolated engine worktree; do not reintroduce engine sources into the Flow-only main checkout.

## Actions on selected text

Add a web toolbar for a native text range: save its exact text and endpoints in the existing Codex focus record, apply a real Typst highlight to safely mapped source text, and draw a temporary red strike through the selected range. Temporary marks use a separate browser overlay and never modify source or request compilation. Keep marks scoped to the document and invalidate their geometry when the marked page changes. Preserve native selection handles and copying.

Prefer the document's callable `highlighted` helper when present, including text-only selections, to retain authored math backgrounds and theme colors. Support complete equations mixed with text through source syntax and rendered glyph coverage; reject partial formulas and reused source slices.
