## ADDED Requirements

### Requirement: Flow-only source repository
The repository SHALL maintain only the native Flow app, packaging, branding, app tests, app documentation, and Flow-specific workflow contracts. The engine implementation, editor integrations, and their build and test workspaces SHALL be excluded from the maintained source tree. Prior source history and applicable license notices SHALL be preserved.

#### Scenario: Build from an app-only checkout
- **WHEN** a compatible external engine executable is supplied to an app-only checkout
- **THEN** the app can be built and tested without an engine source workspace
- **AND** the engine is copied into the app bundle independently of the Flow source revision
- **AND** current preview lifecycle and engine feature arguments are preserved

### Requirement: External engine input
Packaging SHALL accept an explicit engine path or FLOW_ENGINE_PATH and otherwise use the current installed Flow engine. The input SHALL be checked before an existing build output is replaced. The release manifest SHALL identify the external engine by version and hashes in addition to the Flow source revision.

#### Scenario: Reuse an installed engine
- **WHEN** Flow is rebuilt using the current installed engine
- **THEN** the engine sources are not needed
- **AND** the installed app, project settings, and running previews are unchanged by the build

#### Scenario: Reject an incompatible or missing engine
- **WHEN** the supplied engine is missing or lacks the required preview CLI support
- **THEN** packaging reports an actionable error
- **AND** an existing output bundle is preserved

### Requirement: Product repository identity
The development repository SHALL be named tinymist-flow to match the product. Current documentation and local remote URLs SHALL use this name. The earlier independent repository SHALL be retained as tinymist-flow-archive.

#### Scenario: Align repository and product names
- **WHEN** the development repository is renamed to tinymist-flow
- **THEN** its existing history and upstream fork relationship are preserved
- **AND** the earlier independent repository remains accessible as tinymist-flow-archive
- **AND** README generation and development instructions use the current repository name

### Requirement: Branded installed app
The system SHALL install a macOS app named tinymist-flow with its own bundle identifier, icon, bundled engine, and release manifest.

#### Scenario: Build and install
- **WHEN** a local release is built and installed
- **THEN** development rebuilds do not overwrite the installed engine
- **AND** macOS can attribute the app and its preview service to tinymist-flow

### Requirement: Project and service controls
The app SHALL let the user select or add projects, edit preview settings, open previews and logs, start or stop services, and configure app login startup. Profiles SHALL be validated before persistence or launch.

#### Scenario: Start a configured project
- **WHEN** the user starts a project
- **THEN** its preview binds only to loopback using its configured port and original source paths
- **AND** the focus record PID matches its LaunchAgent PID

#### Scenario: Stop a project
- **WHEN** the user stops a project
- **THEN** its job is disabled until explicitly started again

### Requirement: Recoverable updates
Installation SHALL validate the incoming bundle and preserve the previous installed version. A rollback command SHALL restore that bundle and restart previously running profiles. Failures SHALL be reported explicitly.

#### Scenario: Update or rollback
- **WHEN** a user installs a valid new local release or requests rollback
- **THEN** settings and project source files are preserved
- **AND** the installed version and source revision can be inspected

### Requirement: Tail Hosting integration
Migration SHALL import the configured project, keep its port and HTTPS URL, and leave ingress ownership with Tail Hosting. The adapter SHALL preserve a recoverable copy of the previous configuration.

#### Scenario: Migrate an existing preview
- **WHEN** migration is requested for a Tail Hosting registry
- **THEN** lifecycle commands use the installed Flow app and focus readers use the migrated profile's record
- **AND** unrelated hosted services retain their configuration
