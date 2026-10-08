#import "mod.typ": *

#show: book-page.with(title: [Preview Feature])

Two ways of previewing a Typst document are provided:
- PDF Preview: let lsp export your PDF on typed, and open related PDF by your favorite PDF viewer.
- Web (SVG) Preview: use builtin preview feature.

Whenever you can get a web preview feature, it is recommended since it is much faster than PDF preview and provides bidirectional navigation feature, allowing jumping between the source code and the preview by clicking or lsp commands.

Preview uses the same compiler font, package, and certificate environment as ordinary editing and exporting. Configure that shared environment once in #cross-link("/feature/compiler-settings.typ", [Compiler Settings]).

= PDF Preview

For non-vscode clients, Neovim client as an example. One who uses `nvim-lspconfig` can place their configuration in the `servers.tinymist.settings` section. If you want to export PDF on typing and output files in `$root_dir/target` directory, please configure it like that:

```lua
return {
  -- add tinymist to lspconfig
  {
    "neovim/nvim-lspconfig",
    opts = {
      servers = {
        tinymist = {
          settings = {
            exportPdf = "onType",
            outputPath = "$root/target/$dir/$name",
          }
        },
      },
    },
  },
}
```

#pro-tip[
  === VSCode:

  The corresponding configuration should be placed in the `settings.json` file. For example:

  ```json
  {
    "tinymist.exportPdf": "onType",
    "tinymist.outputPath": "$root/target/$dir/$name"
  }
  ```
]

Also see:

- #github-link("/editors/vscode/Configuration.md")[VS Cod(e,ium): Tinymist Server Configuration]
- #github-link("/editors/neovim/Configuration.md")[Neovim: Tinymist Server Configuration]

= Builtin Preview Feature

== Using `tinymist.startDefaultPreview` Command (Since Tinymist v0.13.6) <default-preview>

You can use `tinymist.startDefaultPreview` command to start a preview instance without arguments. This is used for the
case where a client cannot pass arguments to the preview command, e.g. helix. Default Behaviors:
- The preview server listens on a random port.
- The colors are inverted according to the browser (usually also system) settings.
- The preview follows an inferred focused file from the requests from
  the client.
- The preview is opened in the default browser.

You can set the arguments to used by configuration `tinymist.preview.browsing.args` to *override* the default behavior. The default
value is `["--data-plane-host=127.0.0.1:0", "--invert-colors=auto", "--open"]`. Intentionally, the name of the configuration is *not* `tinymist.defaultPreviewArgs` or `tinymist.preview.defaultArgs` to avoid confusion.

== Running preview server in background (Since Tinymist v0.13.6) <background-preview>

You can start a preview instance in background with configuration:
```jsonc
{
  "tinymist.preview.background.enabled": true,
}
```

Default Behaviors:
- The preview server listens on `127.0.0.1:23635`.
- The colors are inverted according to the browser (usually also system) settings.
- The preview follows an inferred focused file from the requests from
  the client.

You can set the arguments to used by configuration `tinymist.preview.background.args` to *override* the default behavior. The default
value is `["--data-plane-host=127.0.0.1:23635", "--invert-colors=auto"]`. Example:

```jsonc
{
  "tinymist.preview.background.args": ["--data-plane-host=127.0.0.1:23635", "--invert-colors=never"],
}
```

== Viewing a preview over Tailnet <tailnet-preview>

For a mobile review workflow, the preview can listen on the development host's Tailscale MagicDNS name while edits continue on that host. From this repository, start it with:

```bash
scripts/tailnet-preview.sh /absolute/path/to/main.typ
```

The launcher prints a stable URL such as `http://my-mac.example-tailnet.ts.net:23625/`. Open that URL in Safari on an iPhone or iPad connected to the same Tailnet with MagicDNS enabled. The preview watches the source files on the development host, so edits written there appear in the mobile browser without running an editor on the mobile device.

The launcher discovers the name from `tailscale status --json`, binds only the browser-facing data plane to the address resolved by that name, and keeps the control plane on loopback. The following environment variables can override its defaults:

- `TINYMIST_BIN`: Tinymist executable to run.
- `TINYMIST_TAILNET_HOST`: exact MagicDNS hostname, skipping CLI discovery.
- `TINYMIST_PREVIEW_PORT`: browser-facing port; the default is `23625`.
- `TAILSCALE_BIN`: Tailscale executable; the default is `tailscale`.

Do not replace the MagicDNS name with `0.0.0.0`. A wildcard listener also exposes the preview on other network interfaces and does not match Tinymist's fixed WebSocket Origin check. The preview server does not authenticate Tailnet peers; use Tailscale ACLs or grants if the document must not be visible to every peer in the Tailnet.

=== HTTPS reverse proxy

When a trusted reverse proxy terminates HTTPS, bind the data plane to loopback and configure its exact public origin with `TINYMIST_ALLOWED_ORIGINS`. For example:

```bash
TINYMIST_ALLOWED_ORIGINS=https://my-mac.example-tailnet.ts.net:23625 \
  tinymist preview main.typ --data-plane-host=127.0.0.1:23625 --no-open
```

This variable accepts a comma-separated list of exact canonical HTTPS origins, with no path, credentials, query, fragment or wildcard. It does not publish a server or enable Funnel. A separate Tailnet-only Tailscale Serve mapping must proxy this port. A macOS background LaunchAgent also needs permission to access a project in the Documents folder.

== CLI Integration

```bash
typst-preview /abs-path/to/main.typ --partial-rendering
```

is equivalent to

```bash
tinymist preview /abs-path/to/main.typ --partial-rendering
```

== Editor Integration

#pro-tip[
  === VSCode:

  The preview feature is integrated into the tinymist extension.
]

#pro-tip[
  === Neovim:

  You may seek #link("https://github.com/chomosuke/typst-preview.nvim")[typst-preview.nvim] for the preview feature.
]

#pro-tip[
  === Emacs:

  You may seek #link("https://github.com/havarddj/typst-preview.el")[typst-preview.el] for the preview feature.
]

== `sys.inputs`

If the document is compiled by lsp, you can use `sys.inputs` to get the preview arguments:

```typ
#let preview-args = json(bytes(sys.inputs.at("x-preview", default: "{}")))
```

There is a `version` field in the `preview-args` object, which will increase when the scheme of the preview arguments is changed.

```typ
#let version = preview-args.at("version", default: 0)
#if version <= 1 {
  assert(preview-args.at("theme", default: "light") in ("light", "dark"))
}
```

== Developer Guide

See #link("https://enter-tainer.github.io/typst-preview/arch.html")[Typst-Preview Developer Guide].

=== Theme-aware template

The only two abstracted theme kinds are supported: `light` and `dark`. You can use the following code to get the theme:

```typ
#let preview-theme = preview-args.at("theme", default: "light")
```

== Bounded continuous scrolling

For long documents on mobile browsers, use `--partial-rendering=true` with document preview mode. Partial SVG preview requests visible pages plus one neighboring page on each side. Offscreen pages retain their dimensions as placeholders and do not allocate fallback canvases. Scroll requests are coalesced; document updates remain ordered. Shared font definitions and the document model remain in memory, so this does not bound total memory independently of document size.

When a source edit changes the paged document, document preview follows the changed location after rendering. It compares page contents and uses source-to-document mapping to place the edited text in view when available; otherwise it moves to the first changed page. Initial loading and reconnecting do not trigger a jump. A jump across a long document is immediate so the browser does not render every intermediate page. If you are actively scrolling when an update arrives, a *Jump to change* button appears instead of moving the page under your finger. Updates that do not change any visible page do not move the preview.

When building a custom frontend, run `node scripts/build.mjs build:preview` before rebuilding the CLI. The workspace uses the local assets crate so the resulting frontend is embedded in the executable.

== Share a selected location with a local assistant

Set `TINYMIST_PREVIEW_FOCUS_FILE` to an absolute local JSON path when starting the standalone CLI. The service creates a waiting record on startup. After the document finishes rendering, tap a sentence, equation, or component. A status line acknowledges the saved page and source location. The record contains the latest explicit tap, its time and compiler revision, and up to 17 surrounding source lines capped at 8000 characters. File permissions are owner-only on Unix.

An assistant with local filesystem access can read this record when you ask about the selected passage. Source lines and columns in the record are one-based. Scrolling does not replace the selection, and the latest tap across connected viewers wins. A changed render revision or unmapped position clears the previous source selection and reports the condition. The source excerpt comes from the compilation snapshot; consumers should check its age and compare it with the current source before editing.

This channel supplies context for a subsequent request. It does not automatically start an assistant response or capture the mobile screen. Without the environment variable, preview retains its existing editor jump behavior.
