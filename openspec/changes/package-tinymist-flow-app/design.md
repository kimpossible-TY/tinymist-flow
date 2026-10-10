## Context

Flow runs a compatible tinymist executable copied into its installed app bundle. The app owns project settings and preview lifecycle; Tail Hosting owns HTTPS ingress. This repository maintains the Swift app, Python packaging, engine implementation and preview frontend together. macOS Documents consent has needed refresh after ad-hoc rebuilds.

## Goals / Non-Goals

Goals: branded app identity, independent installed engine, project profiles, menu bar controls, standard user data paths, recoverable releases, and migration of the existing preview.
Non-goals: replacing tinymist at runtime, a new editor, or an automatic public update service.

## Decisions

- Keep one Flow repository with apps/macos, assets/branding, app packaging and tests, Typst documentation, engine crates, preview frontend, supporting editor workspaces, and OpenSpec artifacts. Restore the engine and preview workspace when integrating preview work, preserving repository naming and upstream licensing.
- Accept an external engine through --engine or FLOW_ENGINE_PATH, defaulting to the current installed app's flow-engine. Check the executable and required preview CLI support before replacing a build output. Record the supplied engine version and input hash separately from the Flow source revision. The existing compatible engine preserves mobile selection, assistant focus, device themes, and change restoration; an arbitrary upstream binary is not assumed to implement this contract.
- Generate the Flow README and development guide with a typlite executable. The app build and app tests do not require Rust, Yarn, or a Node dependency workspace. Tests compile the current Swift sources instead of using an old dist app. Native engine builds use --build-engine with the locked flow-release profile, stage packaging before replacing an existing output, and record engineBuild settings and engine provenance in the release manifest.
- Name the development fork kimpossible-TY/tinymist-flow to match the product. Retain the earlier independent repository as kimpossible-TY/tinymist-flow-archive. Update local origin and flow-archive URLs and current documentation; keep dated historical records intact. Use docs/tinymist/tinymist-flow.typ as the generated README source.
- Compile one Swift executable for GUI and service control. The LaunchAgent calls the app with --serve and a profile ID; it execs the bundled engine so launchd and focus-record PIDs agree. Associate the job with the app bundle identifier.
- Store validated profiles and focus records in Application Support/tinymist-flow, logs in Library/Logs/tinymist-flow. Profiles carry project, entry, fonts, packages, port, and optional public URL. Always bind the backend to loopback.
- Keep Tailscale routes in Tail Hosting. An opt-in adapter imports profiles and delegates lifecycle commands to the installed app. It retains existing labels and ports during migration.
- Build into dist; install only verified bundles into ~/Applications. Preserve the previous bundle before replacement and provide explicit rollback. A manifest records source revision and engine/app hashes. Code signing accepts a configured identity and reports ad-hoc fallback honestly.
- Keep app login startup separate from preview service startup; stopping a preview disables its LaunchAgent until explicitly started.
- Store only the selected certificate's SHA-1 identifier in user Application Support via an explicit configure-signing command. Resolve build identity from --identity, FLOW_SIGN_IDENTITY, the saved configuration, then the existing ad-hoc default. A saved but unavailable certificate is an error. Keep private keys in the user's Keychain; configuration contains no private key or password.
- Sign the app and engine with the same certificate, record its SHA-256 fingerprint in the release manifest, and verify both signatures against that fingerprint. Existing ad-hoc bundles remain verifiable. Use a local self-signed code-signing certificate for this personal Mac; do not introduce general certificate trust or change TCC databases.
- Compare designated requirements across independently built bundles before installation. Initial migration from ad-hoc signing may require OS consent; report permission continuity as verified only after real document delivery survives another installed build.
- After stopping profiles for installation or recovery, poll their status with a bounded deadline before moving the installed bundle. launchd bootout can return while the old PID remains visible; an immediate start can otherwise skip the new job because it still sees that PID.

## Risks / Trade-offs

- Ad-hoc signing can require new Documents consent after updates; stable bundle naming alone does not promise persistent TCC grants. Support a real signing identity when available.
- Updates may fail after installation due to OS consent. Preserve the previous app and report health failure rather than claiming success.
- The app manages only configured projects. Tail Hosting remains responsible for remote HTTPS and access policy.
- External engines remain supported dependencies; the maintained engine can also be built locally. A compatible executable or native engine build is required for a fresh installation; the existing installed engine can supply app rebuilds. This source extraction does not replace or restart the user's installed app.

## Migration Plan

For the original app delivery: build and validate a branded bundle, import the local registry, migrate the preview job, verify document delivery and focus, and retain recovery backups. The earlier source extraction remains recorded in Git history. During integration of preview activity status, restore engine and preview sources, retain external engine packaging and repository naming, combine native engine builds with staged packaging, and validate without changing the installed app.
