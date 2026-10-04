## Context

Flow runs a compatible tinymist executable copied into its installed app bundle. The app owns project settings and preview lifecycle; Tail Hosting owns HTTPS ingress. This repository now maintains the Swift app and Python packaging rather than the engine implementation. macOS Documents consent has needed refresh after ad-hoc rebuilds.

## Goals / Non-Goals

Goals: branded app identity, independent installed engine, project profiles, menu bar controls, standard user data paths, recoverable releases, and migration of the existing preview.
Non-goals: maintaining or building the engine, replacing tinymist at runtime, a new editor, or an automatic public update service.

## Decisions

- Keep one Flow app repository: apps/macos, assets/branding, app packaging and tests, standalone Typst documentation, and Flow-specific OpenSpec artifacts. Remove the Rust workspace, editor integrations, engine fixtures, and unrelated upstream tooling. Preserve the license and prior code in Git history.
- Accept an external engine through --engine or FLOW_ENGINE_PATH, defaulting to the current installed app's flow-engine. Check the executable and required preview CLI support before replacing a build output. Record the supplied engine version and input hash separately from the Flow source revision. The existing compatible engine preserves mobile selection, assistant focus, device themes, and change restoration; an arbitrary upstream binary is not assumed to implement this contract.
- Generate only the Flow README and development guide with an external typlite executable. The app build and app tests do not require Rust, Yarn, or a Node dependency workspace. Tests compile the current Swift sources instead of using an old dist app.
- Name the development fork kimpossible-TY/tinymist-flow to match the product. Retain the earlier independent repository as kimpossible-TY/tinymist-flow-archive. Update local origin and flow-archive URLs and current documentation; keep dated historical records intact. Use docs/tinymist/tinymist-flow.typ as the generated README source.
- Compile one Swift executable for GUI and service control. The LaunchAgent calls the app with --serve and a profile ID; it execs the bundled engine so launchd and focus-record PIDs agree. Associate the job with the app bundle identifier.
- Store validated profiles and focus records in Application Support/tinymist-flow, logs in Library/Logs/tinymist-flow. Profiles carry project, entry, fonts, packages, port, and optional public URL. Always bind the backend to loopback.
- Keep Tailscale routes in Tail Hosting. An opt-in adapter imports profiles and delegates lifecycle commands to the installed app. It retains existing labels and ports during migration.
- Build into dist; install only verified bundles into ~/Applications. Preserve the previous bundle before replacement and provide explicit rollback. A manifest records source revision and engine/app hashes. Code signing accepts a configured identity and reports ad-hoc fallback honestly.
- Keep app login startup separate from preview service startup; stopping a preview disables its LaunchAgent until explicitly started.

## Risks / Trade-offs

- Ad-hoc signing can require new Documents consent after updates; stable bundle naming alone does not promise persistent TCC grants. Support a real signing identity when available.
- Updates may fail after installation due to OS consent. Preserve the previous app and report health failure rather than claiming success.
- The app manages only configured projects. Tail Hosting remains responsible for remote HTTPS and access policy.
- Engine updates are external dependencies. A compatible executable is required for a fresh installation; the existing installed engine can supply rebuilds. This source extraction does not replace or restart the user's installed app.

## Migration Plan

For the original app delivery: build and validate a branded bundle, import the local registry, migrate the preview job, verify document delivery and focus, and retain recovery backups. For source extraction: keep the current installed engine, adapt packaging and documentation, remove unrelated tracked files, validate a fresh checkout with that external engine, and integrate the source change without installing a new live app.
