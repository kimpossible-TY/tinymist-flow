## Context

The browser initializes a WASM renderer before opening its WebSocket. Managed theme variants compile on viewer demand. Compiler status currently reaches only the editor control plane; the document surface reports neither initial work nor failures. Touch selection intentionally pauses DOM updates.

## Goals / Non-Goals

**Goals:** truthful activity from page load to settled document rendering; compiler failures visible to late viewers; unobtrusive localized feedback; reliable reconnect and selection behavior.

**Non-Goals:** estimating progress percentages or remaining time, identifying OS permission prompts from a browser, changing compilation demand or automatically deploying the installed app.

## Decisions

- Include the initial indicator in the HTML with inline base styles so bundle/WASM loading cannot leave an unexplained blank screen. A browser controller owns subsequent labels, safe-area placement and cleanup.
- Retain the latest compiler status in a per-pipeline Tokio watch channel. Each webview sends its initial state and later changes as `compile-status,compiling|success|error`. This handles errors before connection and coalesces obsolete states without changing the editor protocol.
- Separate transport, compiler and pending-render state. Compiler success means waiting for document delivery; only a completed document render clears activity. Viewport-only completion cannot dismiss compilation or errors. Reconnect clears pending render state; disconnected or errored states take precedence over old completion callbacks.
- Add optional rendering/selection/error lifecycle hooks to the shared DOM context. They report real work and held updates without inspecting private state on a polling timer. The indicator does not steal gestures and respects reduced motion.

## Risks / Trade-offs

- New compiler frames reach older frontends → unknown event kinds are ignored by their existing renderer queue; retain all current frames and ordering.
- Compiler and rendering callbacks can interleave → test state precedence and completion before/after compiler success.
- Mobile overlays can obscure controls → use a compact top indicator inside safe-area/visual-viewport bounds, away from bottom selection controls.
- A long wait can reflect OS file access rather than active rendering → use neutral waiting text and elapsed waiting guidance, without claiming a permission diagnosis.

## Migration Plan

Regenerate bundled preview assets and validate frontend/DOM tests, targeted Rust checks and an isolated loopback browser. Installed app delivery is a separate deployment step; preserve the currently installed engine during implementation.
