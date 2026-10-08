## Why

Opening a preview currently leaves an unexplained blank surface while the browser prepares its renderer, connects, waits for compilation and renders the document. Readers need visible, truthful feedback to distinguish normal work from a failed or disconnected preview.

## What Changes

- Show a compact status ring and localized activity text from initial page load through the first completed render.
- Relay actual compiler activity and errors to the browser, including the current state for newly connected viewers.
- Show rendering, selection-held updates, reconnecting and renderer failures; clear activity after completed document rendering.
- Keep the indicator accessible and inside the mobile safe area without blocking reading or selection.

## Capabilities

### New Capabilities

- `preview-activity-status`: browser-visible preparation, connection, compiler and rendering activity.

### Modified Capabilities

None.

## Impact

Preview data-plane actor and compilation watcher, browser frontend, shared DOM rendering callbacks, focused tests and `docs/tinymist/flow-app.typ`. The additive status frame uses the existing WebSocket; no new dependencies or percentage estimates are required.
