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

== Device document theme

Managed previews follow the viewing device's light or dark appearance, including changes after opening the page. The theme selector offers System, Light, and Dark (기기 설정, 라이트, 다크). An override affects only that viewer; choose System to resume device following. The `t` shortcut toggles the native palette. Switching preserves scroll and preview zoom and waits until an active touch text selection is dismissed.

Documents must choose their native palette from `sys.inputs.at("theme", default: "light")`, using the values `"light"` and `"dark"`. PDE and Ewald already follow this contract. Flow compiles both variants on the host and serves them through the same registered port and tailnet URL; it does not invert document colors or edit the source. Documents without this input contract still work but will not change palette. Color-only variants preserve layout most predictably. Each viewer downloads only its selected variant.

For standalone paged preview, enable this with `tinymist preview main.typ --follow-system-theme`. This overrides an explicit `--input theme=...`; unrelated inputs remain unchanged. Without the flag, preview keeps its existing behavior. HTML and bundle outputs do not support this option. Compiling both variants uses additional host memory and CPU.

== Resume the last edited location

Managed previews remember the latest successful visual edit, even while no browser is connected. Opening or refreshing the preview returns to that position after the document renders. A recent reader gesture defers navigation and shows Jump to change (변경 위치로). Switching the document palette keeps the current reading position instead; later live edits still follow their changed locations.

Each profile stores one private `changes/<profile-id>.json` record under the app's Application Support directory, independently of explicit assistant taps. Unchanged output retains its position across service restarts. If output changed while the service was stopped, the first changed page is used instead of stale coordinates. Missing or invalid history establishes a baseline without guessing an earlier edit. Comments or other edits that do not change rendered output do not change the saved position.

Standalone paged preview can opt in by setting `TINYMIST_PREVIEW_CHANGE_FILE` to an absolute path. Records are scoped to the project root and entry file, with separate light and dark positions. This does not save a reader's scroll history or expose a new network endpoint.

= Files and ownership

- App and installed engine: `~/Applications/tinymist-flow.app`
- Profiles and explicit selection records: `~/Library/Application Support/tinymist-flow/`
- Logs: `~/Library/Logs/tinymist-flow/`
- Previous app bundles: Application Support's `releases/` folder
- LaunchAgents: `~/Library/LaunchAgents/`

Flow manages engine execution, project profiles and preview lifecycle. Tail Hosting owns Tailscale HTTPS routes and remote access policy. Changing a local port requires updating its Tail Hosting route. Original Typst source files stay in their project folders.

= Build and install

From the repository:
```sh
python3 scripts/flow-app.py build --version 0.1.0
python3 -m unittest discover -s tests/flow-app -v
python3 scripts/flow-app.py install dist/tinymist-flow.app
```

Packaging uses a compatible external engine. It chooses `--engine /absolute/path/to/flow-engine`, then `FLOW_ENGINE_PATH`, then the engine in the current installed app. A fresh machine needs a compatible engine from an existing Flow bundle. The build checks required preview CLI support and copies the executable into a new app; engine sources are not needed. The installed engine is independent of build output. AppKit and Foundation are the app runtime dependencies; packaging uses the local Swift compiler and Python 3.

The app keeps the upstream engine protocol and VS Code configuration names. For the installed engine, set `tinymist.serverPath` to the absolute path ending in `tinymist-flow.app/Contents/MacOS/flow-engine`. Remote VS Code settings must refer to the remote host's installed path.

= Signing and macOS consent

The app has a stable bundle ID: `io.github.kimpossible-ty.tinymist-flow`. Build with `FLOW_SIGN_IDENTITY` set to a valid code-signing identity when available. Without one the build is signed ad hoc. macOS can require Documents access again after an ad-hoc update. The app name and icon identify the request, but do not replace OS consent. Administrator authentication must be completed by the user when macOS requires it.

= Update and recover

Use a short feature branch, run the relevant tests, merge into `main`, and build from a clean commit. Install the new bundle with the same install command. The installer checks the bundle signature, engine hash and version before replacing anything, backs up the old app, restarts previously running profiles, and checks HTTP startup. If startup fails, it restores the old bundle. Verify document rendering after installation; HTTP alone does not establish document health.

```sh
python3 scripts/flow-app.py verify ~/Applications/tinymist-flow.app
python3 scripts/flow-app.py rollback
```

Rollback restores the previous app while preserving profiles and source files. Keep the backup until the new version is confirmed. Release metadata records the Flow source revision, dirty-worktree flag, external engine version and input hash, bundled engine hash, app version and signing mode. Tag confirmed installed releases as `flow-vX.Y.Z`; retain one code commit per feature and a separate deployment record.

= Tail Hosting migration

```sh
python3 scripts/flow-app.py migrate-tail-hosting /path/to/Tail_hosting
```

This imports project settings, backs up the old registry and launcher, and installs a small lifecycle adapter. Existing labels and ports remain usable by status and focus readers. Activate the imported profile using Restart in Flow. Inspect macOS consent and verify HTTPS document delivery before considering migration complete.

```sh
python3 scripts/flow-app.py restore-tail-hosting
```

The restore command reinstates the old registry, launcher and preview LaunchAgent from the migration backup. Unrelated hosted services are preserved. The adapter does not create or change Tailscale Serve routes.

= Maintaining the Flow app

Use this repository for the app, branding, packaging, app tests, and documentation. Keep app work in `apps/macos`, branding in `assets/branding`, and behavior contracts in OpenSpec. Engine executables are external inputs; the existing compatible engine supplies mobile selection, assistant focus, device themes, and change restoration. Small local fixes can stay lightweight. Preserve upstream copyright, Apache License 2.0, and applicable third-party notices.

Test a replacement engine for compatibility before bundling it, and keep its version and hash alongside the Flow source revision. App work does not require rebuilding or maintaining the language server. The source repository is `kimpossible-TY/tinymist-flow`. Previous engine and editor implementations remain in Git history, and the earlier independent repository is retained as `kimpossible-TY/tinymist-flow-archive`.
