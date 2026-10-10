## Why

Flow needs a stable branded macOS installation, project settings, visible preview controls, and recoverable updates. These app responsibilities can be maintained independently of the engine implementation. The repository maintains Flow, engine and preview sources while supporting compatible external engine packaging and preserving preview features.

## What Changes

- Package tinymist-flow as a native menu bar app with its own icon and bundle identity.
- Manage project profiles, preview lifecycle, login startup, logs, and explicit selection context from the app.
- Install the engine inside the app, with settings and logs in standard user Library locations.
- Add verified local update and rollback commands, release manifests, and installed-version tags.
- Import existing Tail Hosting configuration and keep HTTPS ingress owned by Tail Hosting.
- Document app maintenance, external engine compatibility, deployment, and recovery.
- Align the development repository name and documentation with tinymist-flow, retaining the earlier independent repository as tinymist-flow-archive.
- Maintain the app, engine, preview frontend and supporting workspace sources together; accept an external engine or build a native engine with recorded ThinLTO settings.
- Preserve staged packaging, external engine provenance, freshly compiled app tests, and the repository naming changes when integrating preview work.
- Persist a user-selected signing certificate for app and engine builds, reject missing configured identities instead of falling back to ad-hoc signing, and verify certificate provenance.

## Capabilities

### New Capabilities
- `flow-macos-app`: branded app, project profiles, lifecycle controls, installation and rollback.

### Modified Capabilities

## Impact

Maintained sources include the app, branding, packaging, tests, Typst documentation, engine crates, preview frontend, supporting editor workspaces and the Tail Hosting adapter. Packaging accepts compatible external engines or builds the maintained engine sources; installed engines remain independent of development outputs.
