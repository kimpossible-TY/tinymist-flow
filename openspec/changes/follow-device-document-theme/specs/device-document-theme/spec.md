# Spec Delta

## Purpose

Provide native light and dark document previews that follow each viewer's device preference without changing another viewer's palette or disrupting mobile reading.

## ADDED Requirements

### Requirement: Standalone preview supports native theme variants

Standalone preview SHALL offer an opt-in `--follow-system-theme` mode for paged output using document inputs `theme=light` and `theme=dark` behind the same HTTP port. Existing preview behavior SHALL remain unchanged without the flag. Incompatible export formats SHALL fail with a clear error.

#### Scenario: Two viewers choose different palettes

- **WHEN** a dark-device viewer and a light-device viewer connect to the same themed preview
- **THEN** each receives its native document palette independently
- **AND** source edits update both variants without changing their selected palettes

#### Scenario: Unsupported output

- **WHEN** native theme following is requested for non-paged output
- **THEN** preview exits with an explanatory error rather than serving a misleading theme control

### Requirement: Viewer follows device preference with a local override

The themed frontend SHALL default to System, listen for changes to `prefers-color-scheme: dark`, and expose accessible System, Light, and Dark choices. Native palettes SHALL NOT be additionally inverted with CSS.

#### Scenario: Device switches after page load

- **WHEN** the device preference changes while the viewer is in System mode
- **THEN** the document switches to the new native palette without a page reload

#### Scenario: Viewer selects an override

- **WHEN** a viewer chooses Light or Dark and the device preference changes
- **THEN** the explicit choice remains active
- **AND** returning to System applies the current device preference

### Requirement: Theme switching preserves reading and text selection

Theme switching SHALL preserve the viewer's scroll position, custom zoom, and slide page. While a native touch text range is selected, the frontend SHALL defer theme switching until the selection collapses, then apply the latest requested theme.

#### Scenario: Theme changes while reading

- **WHEN** a viewer switches palette after scrolling and zooming
- **THEN** the updated document remains at the same reading position and custom zoom

#### Scenario: Device changes while text is selected

- **WHEN** a viewer has a selected touch text range and the device changes theme
- **THEN** the current SVG and selection remain intact
- **AND** the latest requested theme is applied after the selection collapses

### Requirement: Theme routes retain preview security and focus semantics

Theme connections SHALL use the existing origin restrictions. Focus sharing SHALL resolve against the selected variant's rendered revision and source context, and SHALL retain a single ordered focus record across viewers.

#### Scenario: Untrusted origin requests dark output

- **WHEN** an untrusted origin requests a theme WebSocket
- **THEN** the connection is rejected under the same policy as the original preview endpoint

#### Scenario: Tap in dark variant

- **WHEN** a viewer taps text in the rendered dark document
- **THEN** focus sharing records the matching source location and revision without resolving against the light document

### Requirement: Managed previews follow the device by default

tinymist-flow SHALL enable native device theme following for its managed paged preview services without changing their registered ports or tailnet URLs.

#### Scenario: Existing profile is restarted

- **WHEN** a managed preview profile starts with the updated launcher
- **THEN** its existing URL offers device-following light and dark document previews
