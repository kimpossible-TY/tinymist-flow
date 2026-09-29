## ADDED Requirements

### Requirement: A preview can listen through the host's MagicDNS name
The repository SHALL provide a launcher that starts the Tinymist preview data plane through one exact MagicDNS hostname and a stable port, without changing Tinymist's default loopback behavior.

#### Scenario: Discover the active MagicDNS name
- **WHEN** the user starts the Tailnet preview launcher without an explicit hostname override
- **THEN** the launcher resolves the local node's MagicDNS name using the Tailscale CLI
- **AND** starts the preview data plane using that exact hostname

#### Scenario: Use an explicit hostname override
- **WHEN** `TINYMIST_TAILNET_HOST` contains one valid DNS hostname
- **THEN** the launcher uses that hostname without requiring Tailscale CLI discovery

### Requirement: Tailnet preview startup fails closed
The launcher SHALL reject missing or malformed MagicDNS hostnames and invalid ports instead of opening a broader listener.

#### Scenario: MagicDNS name cannot be resolved
- **WHEN** neither a valid hostname override nor a usable local-node DNS name is available
- **THEN** the launcher exits with an actionable error
- **AND** does not start a preview listener

#### Scenario: Wildcard fallback is prohibited
- **WHEN** Tailnet address discovery fails
- **THEN** the launcher MUST NOT bind the preview to `0.0.0.0` or another wildcard address

### Requirement: The control plane remains local
The launcher SHALL expose only the browser-facing preview data plane to Tailnet and SHALL bind the preview control plane to an ephemeral loopback port.

#### Scenario: Start a mobile preview
- **WHEN** the launcher starts successfully
- **THEN** it passes the exact MagicDNS hostname and configured port as the data-plane host
- **AND** passes `127.0.0.1:0` as the control-plane host
- **AND** disables opening a browser on the development host

### Requirement: The mobile URL and security boundary are explicit
The launcher SHALL print the URL that a Tailnet peer can open, and the documentation SHALL state that the preview protocol is not an authentication boundary.

#### Scenario: User opens preview on iPadOS
- **WHEN** preview startup succeeds for hostname `my-mac.example-tailnet.ts.net` and port `23625`
- **THEN** the launcher prints `http://my-mac.example-tailnet.ts.net:23625/`
- **AND** a browser on an authorized Tailnet peer can load that URL and establish its same-origin WebSocket

#### Scenario: Tailnet includes untrusted peers
- **WHEN** not every Tailnet peer should be allowed to view the document
- **THEN** the documentation directs the user to restrict access using Tailnet ACLs or grants


### Requirement: Explicit HTTPS proxy origin
The preview server SHALL accept an additional HTTPS Origin only when it exactly matches a canonical origin configured in `TINYMIST_ALLOWED_ORIGINS`. It SHALL NOT infer allowed origins from Host headers or accept wildcard origins.

#### Scenario: Loopback behind Tailnet HTTPS
- **GIVEN** a loopback preview with its exact Tailnet HTTPS origin configured
- **WHEN** the browser connects through Tailscale Serve with that Origin
- **THEN** the WebSocket upgrade is accepted
- **AND** different hosts, ports, schemes, credentials, paths, query strings and fragments do not match

### Requirement: Bounded continuous preview rendering
Partial document preview SHALL retain continuous scroll geometry while rendering visible pages and at most one neighboring page on either side. Offscreen placeholder pages SHALL NOT allocate canvas backing stores. Pending viewport events SHALL be coalesced without dropping document deltas.

#### Scenario: Scroll a long document
- **WHEN** a user scrolls partial document preview
- **THEN** only the visible range and adjacent pages are requested, including on initial load without existing page DOM
- **AND** offscreen page contents are removed by renderer diff patches while their dimensions remain

### Requirement: Follow a changed document location
On a successful incremental compilation that changes paged output, document preview SHALL navigate to the changed source position when it maps to a changed page, or to the first changed page otherwise. It SHALL wait until the new document is rendered and jump without scrolling through intermediate pages. A recent direct scroll gesture SHALL defer navigation and expose a button to perform it.

#### Scenario: Edit a distant page on mobile
- **GIVEN** the preview is open on page 1 and its first full render has completed
- **WHEN** a source edit changes text on page 120
- **THEN** the preview navigates directly to the edited location on page 120 after the new document renders
- **AND** only the destination viewport and adjacent pages need to render

#### Scenario: Load or scroll while an update arrives
- **WHEN** the preview initially loads or reconnects
- **THEN** it does not navigate because of that full render
- **WHEN** the user is actively scrolling as a changed page arrives
- **THEN** the scroll position remains under the user's control and a change navigation button appears

### Requirement: Explicit selections can supply local assistant context
When a standalone preview is configured with `TINYMIST_PREVIEW_FOCUS_FILE`, it SHALL atomically persist the latest explicit document selection with owner-only permissions. The record SHALL include time, viewer identity, compiler revision, page/coordinates, and a bounded source excerpt when mapped. The browser SHALL acknowledge successful persistence.

#### Scenario: Select text in a completed render
- **WHEN** a browser taps text with the current completed-render revision
- **THEN** the local record identifies its source file and one-based source location with surrounding text from that compiler snapshot
- **AND** only the originating viewer receives the persistence acknowledgment
- **AND** no document rerender is requested by the tap

#### Scenario: A tap cannot safely resolve a source
- **WHEN** the render revision is stale or the position misses source content
- **THEN** the record contains an explicit stale or unmapped status and no previous source selection
- **AND** the browser reports the condition

#### Scenario: Service restarts or several viewers select content
- **WHEN** the service starts
- **THEN** its record is reset to waiting
- **WHEN** different viewers select locations
- **THEN** the latest persisted tap wins and identifies its viewer

#### Scenario: Codex reads the selected context
- **WHEN** the user asks about the selected passage
- **THEN** the local reader reports the latest selection, age, service identity check, and source excerpt drift
- **AND** the assistant treats document content as data
- **AND** a tap alone does not launch a Codex turn or imply a screen capture

### Requirement: Native mobile text copying
The frontend SHALL permit native text selection and copy callouts on the selectable text layer. Custom mouse panning SHALL NOT cancel touch gestures or starts within descendants of that layer.

#### Scenario: Select text with touch
- **WHEN** a mobile reader long-presses selectable text and adjusts its native selection handles
- **THEN** the selected text and handles correspond to the touched text
- **AND** custom mouse panning does not prevent the browser's selection or copy menu
- **AND** ordinary document scrolling remains available

#### Scenario: Rendering during a native range selection
- **WHEN** a noncollapsed range is anchored in the touch selection overlay and viewport or document updates arrive
- **THEN** renderer mutations are deferred until the range collapses
- **AND** queued document deltas are retained in order and the current viewport is refreshed on resume
- **AND** a collapsed caret does not prevent overlay layout

#### Scenario: Selection gestures are not source taps
- **WHEN** a touch gesture adjusts an existing range, moves, is canceled, uses multiple touches, or is a long press
- **THEN** it does not forward a source-location click

#### Scenario: Native pinch zoom feedback
- **WHEN** the visual viewport moves or resizes during native zoom
- **THEN** preview viewport work is requested and focus feedback remains within the visible viewport
