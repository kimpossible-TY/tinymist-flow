#set page(paper: "a4", margin: 22mm)
#set text(size: 10pt)
#set heading(numbering: "1.")
#align(center)[
  #image("../../assets/branding/tinymist-flow.png", width: 25mm)
  #text(size: 24pt, weight: "bold")[tinymist-flow]
  #parbreak()
  Personal Typst workspace for macOS
]

= Everyday use

Open `~/Applications/tinymist-flow.app`. The Flow menu appears in the menu bar. Choose a project, then Start and Open Preview. Add Project selects a Typst entry file; adjust the project root if the entry belongs to a larger workspace. Font paths are relative to that root and separated by semicolons. The optional HTTPS URL is the address configured by Tail Hosting.

Stop disables the project's LaunchAgent until Start is used again. Closing or quitting the menu bar app leaves running previews active. The login checkbox opens the menu bar app after login. Started preview services also resume after login; sleeping or logging out can interrupt availability.

The status distinguishes a stopped process from a responding HTTP server. HTTP readiness does not prove successful compilation: inspect the preview and logs for compiler errors. After restarting, tap the document again before asking Codex about a selected passage.

On touch devices, selecting text temporarily holds preview rendering so the visual page and native selection stay on the same revision. Dismiss the selection to apply pending edits and refresh the visible pages. Native copying covers the currently populated preview pages, not an arbitrary range across the whole document. A drag or long press does not share a source location with Codex; dismiss the selection and use a short tap to share one.

= Files and ownership

- App and installed engine: `~/Applications/tinymist-flow.app`
- Profiles and explicit selection records: `~/Library/Application Support/tinymist-flow/`
- Logs: `~/Library/Logs/tinymist-flow/`
- Previous app bundles: Application Support's `releases/` folder
- LaunchAgents: `~/Library/LaunchAgents/`

Flow owns compilation, project profiles and preview lifecycle. Tail Hosting owns Tailscale HTTPS routes and remote access policy. Changing a local port requires updating its Tail Hosting route. Original Typst source files stay in their project folders.

= Build and install

From the repository:
```sh
node scripts/build.mjs build:preview
cargo build --locked --release --bin tinymist
python3 scripts/flow-app.py build --version 0.1.0
python3 scripts/flow-app.py install dist/tinymist-flow.app
```

The app's installed engine is a copy. Rebuilding `target/release/tinymist` does not replace the running installation. AppKit and Foundation are the only app runtime dependencies; packaging uses the local Swift compiler and Python 3.

The app keeps the upstream engine protocol and VS Code configuration names. For the installed engine, set `tinymist.serverPath` to the absolute path ending in `tinymist-flow.app/Contents/MacOS/flow-engine`. Remote VS Code settings must refer to the remote host's installed path.

= Signing and macOS consent

The app has a stable bundle ID: `io.github.kimpossible-ty.tinymist-flow`. Build with `FLOW_SIGN_IDENTITY` set to a valid code-signing identity when available. Without one the build is signed ad hoc. macOS can require Documents access again after an ad-hoc update. The app name and icon identify the request, but do not replace OS consent. Administrator authentication must be completed by the user when macOS requires it.

= Update and recover

Use a short feature branch, run the relevant tests, merge into `main`, and build from a clean commit. Install the new bundle with the same install command. The installer checks the bundle signature, engine hash and version before replacing anything, backs up the old app, restarts previously running profiles, and checks HTTP startup. If startup fails, it restores the old bundle. Verify document rendering after installation; HTTP alone does not establish document health.

```sh
python3 scripts/flow-app.py verify ~/Applications/tinymist-flow.app
python3 scripts/flow-app.py rollback
```

Rollback restores the previous app while preserving profiles and source files. Keep the backup until the new version is confirmed. Release metadata records the source revision, dirty-worktree flag, engine hash, version and signing mode. Tag confirmed installed releases as `flow-vX.Y.Z`; retain one code commit per feature and a separate deployment record.

= Tail Hosting migration

```sh
python3 scripts/flow-app.py migrate-tail-hosting /path/to/Tail_hosting
```

This imports project settings, backs up the old registry and launcher, and installs a small lifecycle adapter. Existing labels and ports remain usable by status and focus readers. Activate the imported profile using Restart in Flow. Inspect macOS consent and verify HTTPS document delivery before considering migration complete.

```sh
python3 scripts/flow-app.py restore-tail-hosting
```

The restore command reinstates the old registry, launcher and preview LaunchAgent from the migration backup. Unrelated hosted services are preserved. The adapter does not create or change Tailscale Serve routes.

= Maintaining the personal fork

Use one repository for the engine, app, assets and deployment tools. Keep new app work in `apps/macos`, branding in `assets/branding`, and behavior contracts in OpenSpec. Small local fixes can stay lightweight. Preserve upstream copyright, Apache License 2.0, and third-party notices.

Fetch upstream deliberately and inspect changes before merging or cherry-picking. Prefer targeted fixes and record their upstream commits. Test engine changes separately from app packaging. Internal crate names and protocol identifiers remain compatible to reduce merge conflicts. The active source repository is `kimpossible-TY/tinymist-flow`, matching the product name. The earlier separate repository is retained as `kimpossible-TY/tinymist-flow-archive` for historical storage.
