# Design

## Context

See proposal.md for motivation. Standalone preview currently has one compiler output and one WebSocket sender. The source documents already choose their palette using `sys.inputs.theme`. Mobile preview must not mutate the rendered SVG while a native text range is selected.

## Goals / Non-Goals

**Goals:** independent viewer palettes on one existing URL, live source updates in both palettes, safe reconnection with retained reading state, unchanged origin and focus guarantees.

**Non-Goals:** CSS inversion as a substitute for document palettes, editing document source, automatic discovery of arbitrary theme input names, HTML/bundle export support, persistent user preferences, changing VS Code preview defaults.

## Decisions

- Add `--follow-system-theme` only to standalone preview. Force the input `theme=light` for the primary project and `theme=dark` for a dedicated compiler project sharing fonts, registry, and VFS resources. Register independent preview builders for both projects. A global mutable theme would let one viewer change everyone else's document; two processes would duplicate resource loading and complicate port ownership.
- Route theme WebSockets through the existing HTTP listener. The original server helper remains compatible, while the themed helper accepts explicit light/dark paths. Existing origin validation runs before routing and upgrade. The frontend receives a capability placeholder rather than probing an unsupported route.
- Share the focus store between builders, including its sequence and write lock. Rendering and compile channels remain separate, so a tap resolves against the frame and source context for that viewer's palette.
- A frontend controller owns the media-query listener and accessible System / Light / Dark selector. The default is System. Native theme mode suppresses CSS inversion. System changes affect only auto mode; overrides are local to a viewer. Reconnection retains scroll, custom scale, and slide page. Superseded connections are disposed and queued rendering is prevented after disposal.
- Delay requested transitions while a touch text range exists. On selection collapse, apply the latest desired palette; do not clear or replace the selection to force a transition.
- Managed tinymist-flow profiles enable the CLI option. Other callers remain opt-in.

## Risks / Trade-offs

- Extra host compilation and memory → opt-in; use dedicated projects with shared resource infrastructure and keep mobile clients on only one stream.
- Async transitions can leak handlers or restore the wrong reading state → serialized, coalesced connection changes, explicit disposal, lifecycle tests.
- Palette changes can affect layout in custom documents → preserve scroll and zoom, recommend color-only theme variants; do not claim identical source geometry.
- Ad-hoc app replacement can require renewed macOS Documents consent → validate a staged build first, install once from a clean commit, verify both managed services, and report any OS consent blocker without cycling installations.

## Migration Plan

Build and test frontend, Rust CLI, and launcher. Exercise both palettes concurrently on a temporary preview port and verify edits and focus sharing. Build the bundled app from a clean implementation commit, replace it once, then verify the existing tailnet URLs. Rollback uses the saved previous bundle if an actual functional regression occurs; an OS consent prompt is handled as consent, not by repeated replacement.
