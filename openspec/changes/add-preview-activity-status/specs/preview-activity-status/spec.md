## ADDED Requirements

### Requirement: Initial preview work is visible

The preview SHALL display an accessible localized status ring from initial HTML load through renderer preparation, connection, document waiting and document rendering. It SHALL hide the activity after successful document rendering and SHALL NOT invent percentages.

#### Scenario: First document opens
- **WHEN** a reader opens a preview before the first document is ready
- **THEN** a visible ring and text identify the current known activity
- **AND** the activity disappears after the document renders successfully

### Requirement: Compiler status reaches every viewer

Each preview pipeline SHALL relay its latest compiler state and subsequent changes to its viewers. Compiler success SHALL NOT imply completed browser rendering. Compiler errors SHALL remain visible until new compilation starts or succeeds; rendering a cached previous document SHALL NOT dismiss them.

#### Scenario: New viewer joins a failed compilation
- **WHEN** compilation has failed before a browser connects
- **THEN** that browser receives the current error and displays it without waiting indefinitely

#### Scenario: Compiler success precedes document delivery
- **WHEN** the server reports success but the browser has not received its document
- **THEN** the preview continues to show document waiting activity

### Requirement: Activity follows rendering and connection lifetime

The preview SHALL show reconnecting after disconnection, a static error after renderer failure and selection-held activity when touch selection pauses pending updates. Viewport callbacks SHALL NOT clear unrelated compilation or connection states, and disposed sessions SHALL NOT update their replacement's status.

#### Scenario: Selection holds an update
- **WHEN** a new document frame arrives during native touch selection
- **THEN** status explains that releasing selection allows the update to continue
- **AND** successful rendering after release clears activity

#### Scenario: Connection closes during rendering
- **WHEN** the WebSocket closes before rendering finishes
- **THEN** reconnecting remains visible despite an old render completion

### Requirement: Status remains accessible on mobile

The indicator SHALL remain within the mobile visible safe area, SHALL allow document gestures through it, SHALL announce activity politely and SHALL respect reduced-motion preferences.

#### Scenario: Reader zooms or rotates
- **WHEN** the visual viewport changes while activity is visible
- **THEN** the indicator remains readable inside that viewport without modifying document scale or selection
