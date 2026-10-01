# Proposal

## Why

The always-on previews currently render only the light document palette even when a phone or tablet uses dark mode. PDE and Ewald already support `sys.inputs.theme`, so the preview should use their native palettes and react to device changes without affecting other viewers.

## What Changes

- Add an opt-in standalone preview mode that compiles light and dark document variants behind the existing port.
- Follow `prefers-color-scheme` on load and when it changes, with System / Light / Dark overrides per viewer.
- Preserve reading position and zoom during switching and defer changes while text is selected.
- Enable this mode for tinymist-flow managed previews; retain existing preview defaults elsewhere.
- Document the input contract and validate live updates, routing, isolation, and mobile selection safety.

## Capabilities

### New Capabilities

- `device-document-theme`: Native document theme selection driven by the viewer's device, with independent per-viewer overrides and safe transitions.

### Modified Capabilities

None. The accepted `previewer-provider` capability covers editor provider resolution, not document palettes.

## Impact

- Standalone CLI, preview HTTP routing, preview builder, browser frontend, and macOS launcher arguments.
- Two compilation variants increase host-side work when the option is enabled; fonts and project resources remain shared.
- The existing tailnet URL, port, origin restrictions, focus sharing, and document sources remain unchanged.
- User documentation is maintained in `docs/tinymist/flow-app.typ`.
