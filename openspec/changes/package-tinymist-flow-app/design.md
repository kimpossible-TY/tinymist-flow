## Context

The engine currently runs from target/release and the local Tail Hosting registry owns its LaunchAgent. macOS Documents consent has needed refresh after ad-hoc rebuilds. A native Swift/AppKit shell can provide a small personal app without another UI framework or runtime dependency.

## Goals / Non-Goals

Goals: branded app identity, independent installed engine, project profiles, menu bar controls, standard user data paths, recoverable releases, and migration of the existing preview.
Non-goals: renaming upstream crate/protocol identifiers, a new editor, or an automatic public update service.

## Decisions

- Keep one repository. Add apps/macos and assets/branding. Preserve upstream license and engine interfaces.
- Compile one Swift executable for GUI and service control. The LaunchAgent calls the app with --serve and a profile ID; it execs the bundled engine so launchd and focus-record PIDs agree. Associate the job with the app bundle identifier.
- Store validated profiles and focus records in Application Support/tinymist-flow, logs in Library/Logs/tinymist-flow. Profiles carry project, entry, fonts, packages, port, and optional public URL. Always bind the backend to loopback.
- Keep Tailscale routes in Tail Hosting. An opt-in adapter imports profiles and delegates lifecycle commands to the installed app. It retains existing labels and ports during migration.
- Build into dist; install only verified bundles into ~/Applications. Preserve the previous bundle before replacement and provide explicit rollback. A manifest records source revision and engine/app hashes. Code signing accepts a configured identity and reports ad-hoc fallback honestly.
- Keep app login startup separate from preview service startup; stopping a preview disables its LaunchAgent until explicitly started.

## Risks / Trade-offs

- Ad-hoc signing can require new Documents consent after updates; stable bundle naming alone does not promise persistent TCC grants. Support a real signing identity when available.
- Updates may fail after installation due to OS consent. Preserve the previous app and report health failure rather than claiming success.
- The app manages only configured projects. Tail Hosting remains responsible for remote HTTPS and access policy.

## Migration Plan

Build and validate a branded bundle; install it without touching the current process; import the local registry; verify the app UI; migrate the preview job and refresh Documents consent; confirm HTTPS document delivery and PID-matched focus; retain backups and tag the deployed revision.
