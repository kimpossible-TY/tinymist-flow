## Why

The personal fork now owns mobile preview, editing performance, and Codex context sharing, but deployment still executes a development build named Tinymist. A branded macOS app needs a stable installation, project settings, visible service controls, and recoverable updates.

## What Changes

- Package tinymist-flow as a native menu bar app with its own icon and bundle identity.
- Manage project profiles, preview lifecycle, login startup, logs, and explicit selection context from the app.
- Install the engine inside the app, with settings and logs in standard user Library locations.
- Add verified local update and rollback commands, release manifests, and installed-version tags.
- Import existing Tail Hosting configuration and keep HTTPS ingress owned by Tail Hosting.
- Document personal-fork workflow, upstream maintenance, deployment, and recovery.

## Capabilities

### New Capabilities
- `flow-macos-app`: branded app, project profiles, lifecycle controls, installation and rollback.

### Modified Capabilities

## Impact

New `apps/macos`, `assets/branding`, packaging and validation scripts, project README source, and a migration adapter for the existing local Tail Hosting installation. Engine crates and editor protocol identifiers remain compatible.
