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

Opening a preview shows a small status ring while the browser prepares the renderer, connects to the server, waits for the document, compiles or draws its pages. The ring disappears once the document has rendered. Compiler and rendering failures show a static error; a dropped connection shows reconnection activity. If selection holds an update, the status asks you to release the selection. A long wait also suggests checking the server. These are activity states, not estimated percentages or proof of a macOS permission prompt.

On touch devices, selecting text temporarily holds preview rendering so the visual page and native selection stay on the same revision. Dismiss the selection to apply pending edits and refresh the visible pages. Native copying covers the currently populated preview pages, not an arbitrary range across the whole document. A drag or long press does not automatically share a source location with Codex; use Save for Codex in the selection toolbar to save the exact text, or dismiss the selection and use a short tap to share a location.

== Selected text actions

Select text with the native handles, then use the web toolbar near the bottom of the screen. Highlight uses the source module's callable `highlighted` helper when available, writing `#highlighted[...]` so text and complete equations retain the document's background and theme handling. Without this helper, literal text uses `#highlight[...]` and equations cannot be highlighted automatically. This changes the Typst source and the ordinary watcher recompiles it, so the highlight also appears in exports. Rendered glyph coverage verifies complete equations even when their notation differs from the source. Selections anchored in source text or equations can also include whole inline references, including a scoped `ref` helper's generated equation number; an isolated compiler render verifies the complete selected output. Partial equations or references, reused source content, other code or markup, different source files and changed source snapshots cannot be edited automatically. The candidate document must compile before any source is saved. A rejected highlight keeps the selection available, so Save for Codex can supply its rendered text without selecting it again.

A custom helper must accept a single text element or equation as well as mixed content. When iterating its body, use `if body.has("children") { body.children } else { (body,) }` so singleton content does not cause a missing-field error.

Save for Codex records up to 8000 selected characters and their page endpoints alongside the existing source context. It acknowledges persistence and does not start a Codex turn or compile the document. A newer compiler revision requires selecting again.

Red strike draws a temporary red line across each selected fragment without changing source or compiling. Undo removes the latest mark and Clear marks removes this document's temporary marks in this browser. Marks are stored separately by document in this browser and survive refresh, theme switches and scrolling through unchanged virtual pages. A page whose text or layout changes loses its old marks to avoid displaying them over the wrong passage. Browser storage may be unavailable in private browsing; memory-only marks then last until the view is closed. Temporary marks are not included in PDF exports or synchronized to other devices.

== Device document theme

Managed previews follow the viewing device's light or dark appearance, including changes after opening the page. The theme selector offers System, Light, and Dark (기기 설정, 라이트, 다크). An override affects only that viewer; choose System to resume device following. The `t` shortcut toggles the native palette. Switching preserves scroll and preview zoom and waits until an active touch text selection is dismissed.

Documents must choose their native palette from `sys.inputs.at("theme", default: "light")`, using the values `"light"` and `"dark"`. PDE and Ewald already follow this contract. Flow keeps independent variants and compiles each while viewers are connected to it. Both use the same registered port and tailnet URL. Documents without this input contract still work but will not change palette. Color-only variants preserve layout most predictably. Each viewer downloads only its selected variant.

For standalone paged preview, enable this with `tinymist preview main.typ --follow-system-theme`. This overrides an explicit `--input theme=...`; unrelated inputs remain unchanged. Without the flag, preview keeps its existing behavior. HTML and bundle outputs do not support this option. When both palettes have viewers, compiling both variants uses additional host memory and CPU.

== Resume the last edited location

Managed previews remember the latest successfully rendered visual edit. Edits made while a palette has no viewers are compiled when a viewer returns and then update its remembered position. Opening or refreshing the preview returns to that position after the document renders. A recent reader gesture defers navigation and shows Jump to change (변경 위치로). Switching the document palette or reconnecting an already rendered viewer keeps the current reading position instead; a replayed saved edit does not show a stale jump button, and later live edits still follow their changed locations.

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

Packaging uses a compatible external engine. It chooses `--engine /absolute/path/to/flow-engine`, then `FLOW_ENGINE_PATH`, then the engine in the current installed app. For this path, a fresh machine needs a compatible engine from an existing Flow bundle; it can also build the maintained engine as shown below. The build checks required preview CLI support and copies the executable into a new app; engine sources are not needed for this path. The installed engine is independent of build output. AppKit and Foundation are the app runtime dependencies; packaging uses the local Swift compiler and Python 3.

To build the restored engine and preview frontend from source instead:

```sh
node scripts/build.mjs build:preview
python3 scripts/flow-app.py build --build-engine --jobs 2 --version 0.1.0
```

The native engine build uses the `flow-release` profile with ThinLTO. Its target, profile, Rust flags and any LTO override are recorded in `engineBuild` alongside the engine version and hashes. Use a native ARM Python environment on Apple Silicon. The build copies its result into the staged app and preserves an existing output bundle if packaging fails.

Managed previews compile the light and dark variants when viewers connect to them. Source updates are retained while a variant has no viewers; reconnecting requests its current document. Multiple viewers of the same palette share one compilation. The host classifies interactive CPU work and cache maintenance separately for macOS scheduling.

The app keeps the upstream engine protocol and VS Code configuration names. For the installed engine, set `tinymist.serverPath` to the absolute path ending in `tinymist-flow.app/Contents/MacOS/flow-engine`. Remote VS Code settings must refer to the remote host's installed path.

= Signing and macOS consent

The app has a stable bundle ID: `io.github.kimpossible-ty.tinymist-flow`. Configure a persistent code-signing identity or set `FLOW_SIGN_IDENTITY`. Without either, the build is signed ad hoc. macOS can require Documents access again after an ad-hoc update. The app name and icon identify the request, but do not replace OS consent. Administrator authentication must be completed by the user when macOS requires it.

For repeated personal builds, create one code-signing certificate using Keychain Access's Certificate Assistant or use an existing Apple signing identity. Retain the certificate and its private key in Keychain, then save its identity once:

```sh
python3 scripts/flow-app.py configure-signing --identity "tinymist-flow Local Signing"
python3 scripts/flow-app.py build --version 0.1.9
```

The first signing probe may ask for Keychain access. Complete authentication locally and select Always Allow for `/usr/bin/codesign` to reuse this key. Configuration is saved only after the probe succeeds; cancelling leaves the previous configuration intact.

The saved configuration is `~/Library/Application Support/tinymist-flow/signing.json`; it contains the public certificate identifier, not a private key or password. Builds choose `--identity`, then `FLOW_SIGN_IDENTITY`, then the saved identity. A missing configured identity stops the build instead of falling back to ad-hoc signing. Both the app and bundled engine use the same certificate; the release manifest records its SHA-256 fingerprint, which bundle verification checks for both signatures. A personal self-signed certificate establishes a local identity; it does not provide Apple notarization or public distribution approval.

The initial switch from ad-hoc signing may require one new Documents approval. Keep the same certificate for later builds. Verify document delivery after a subsequent installed build before treating permission continuity as confirmed. Certificate replacement or loss may require consent again.

= Update and recover

Use a short feature branch, run the relevant tests, merge into `main`, and build from a clean commit. Install the new bundle with the same install command. The installer checks the bundle signature, engine hash and version before replacing anything, waits for running profiles to stop, backs up the old app, restarts those profiles, and checks HTTP startup. If startup fails, it restores the old bundle. Verify document rendering after installation; HTTP alone does not establish document health.

An ad-hoc update can wait for macOS Documents consent before opening its HTTP listener. To allow time for that prompt, defer live health verification during the single installation:

```sh
python3 scripts/flow-app.py install dist/tinymist-flow.app --defer-health-check
```

This option retains the new bundle while consent is pending. Bundle validation, backups, and recovery from replacement or service-control errors still apply. After completing consent, verify the running profile, its HTTP endpoint, and delivery of the rendered document. The installer reports that this live verification remains pending.

```sh
python3 scripts/flow-app.py verify ~/Applications/tinymist-flow.app
python3 scripts/flow-app.py rollback
```

Rollback restores the previous app while preserving profiles and source files. Keep the backup until the new version is confirmed. Release metadata records the Flow source revision, dirty-worktree flag, engine version and input hash, bundled engine hash, app version and signing mode. Tag confirmed installed releases as `flow-vX.Y.Z`; retain one code commit per feature and a separate deployment record.

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

Use this repository for the app, engine, preview frontend, branding, packaging, tests, and documentation. Keep app work in `apps/macos`, branding in `assets/branding`, and behavior contracts in OpenSpec. Package a compatible external engine or build the maintained engine sources. Both paths supply mobile selection, assistant focus, device themes, change restoration, selected text actions, and preview activity status. Small local fixes can stay lightweight. Preserve upstream copyright, Apache License 2.0, and applicable third-party notices.

Test a replacement engine for compatibility before bundling it, and keep its version and hash alongside the Flow source revision. App work does not require rebuilding or maintaining the language server. The source repository is `kimpossible-TY/tinymist-flow`. Engine and preview implementations are maintained in this tree; the earlier independent repository is retained as `kimpossible-TY/tinymist-flow-archive`.
