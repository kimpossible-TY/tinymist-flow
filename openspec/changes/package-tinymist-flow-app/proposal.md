## Why

Flow needs a stable branded macOS installation, project settings, visible preview controls, and recoverable updates. These app responsibilities can be maintained independently of the engine implementation. The repository should own only Flow sources while continuing to bundle the existing compatible tinymist executable and preserve its preview features.

## What Changes

- Package tinymist-flow as a native menu bar app with its own icon and bundle identity.
- Manage project profiles, preview lifecycle, login startup, logs, and explicit selection context from the app.
- Install the engine inside the app, with settings and logs in standard user Library locations.
- Add verified local update and rollback commands, release manifests, and installed-version tags.
- Import existing Tail Hosting configuration and keep HTTPS ingress owned by Tail Hosting.
- Document app maintenance, external engine compatibility, deployment, and recovery.
- Align the development repository name and documentation with tinymist-flow, retaining the earlier independent repository as tinymist-flow-archive.
- Manage only Flow app sources, packaging, branding, app tests, and app documentation; consume the existing compatible preview engine as an external binary.
- Remove the engine workspace, editor integrations, unrelated fixtures, documentation, and release tooling from the maintained tree.

## Capabilities

### New Capabilities
- `flow-macos-app`: branded app, project profiles, lifecycle controls, installation and rollback.

### Modified Capabilities

## Impact

Maintained sources are `apps/macos`, `assets/branding`, app packaging and tests, standalone Typst documentation, and the Tail Hosting adapter. The engine is an external executable with its existing preview contract. Engine crates, editor extensions, and unrelated upstream workspaces and release tooling are removed from the maintained tree.
