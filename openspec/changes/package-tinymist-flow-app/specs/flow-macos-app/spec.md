## ADDED Requirements

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
