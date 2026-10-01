/// Import stylesheets for different components
// todo: refactor them, but we don't touch them in this PR
import "./typst.css";
import "./styles/toolbar.css";
import "./styles/layout.css";
import "./styles/help-panel.css";
import "./styles/outline.css";

import { wsMain, PreviewMode } from "./ws";
import { setupDrag } from "./drag";
import { hasTouchSelection } from "typst-dom/touch-selection.mjs";
import {
  DocumentThemeController,
  themeWebSocketUrl,
  captureReadingState,
  createConnectionQueue,
} from "./document-theme";

const windowElem = document.getElementById("typst-container");
windowElem.documents = [];

/// Main entry point of the frontend program.
main();

function main() {
  const wsArgs = retrieveWsArgs();
  const { nextWs } = buildWs();
  window.onload = () => {
    if (!wsArgs.nativeTheme) return nextWs(wsArgs);
    const select = document.createElement("select");
    select.className = "typst-theme-selector";
    const korean = navigator.language.startsWith("ko");
    select.setAttribute("aria-label", korean ? "문서 테마" : "Document theme");
    for (const [value, label] of [
      ["system", korean ? "기기 설정" : "System"],
      ["light", korean ? "라이트" : "Light"],
      ["dark", korean ? "다크" : "Dark"],
    ]) {
      select.add(new Option(label, value));
    }
    document.body.appendChild(select);
    const controller = new DocumentThemeController(
      window.matchMedia("(prefers-color-scheme: dark)"),
      document,
      () => hasTouchSelection(document.getElementById("typst-app")),
      (theme) => {
        document.documentElement.dataset.documentTheme = theme;
        document.documentElement.style.removeProperty("--typst-preview-background-color");
        nextWs({
          ...wsArgs,
          url: themeWebSocketUrl(wsArgs.url, theme),
          onToggleTheme: () => {
            select.value =
              document.documentElement.dataset.documentTheme === "dark" ? "light" : "dark";
            controller.setPreference(select.value);
          },
        });
      },
    );
    select.addEventListener("change", () => controller.setPreference(select.value));
    window.addEventListener("pagehide", (event) => {
      // A bfcache page must retain its listeners for restoration.
      if (!event.persisted) controller.dispose();
    });
  };
  setupVscodeChannel(nextWs);
  setupDrag();
}

/// Placeholders for typst-preview program initializing frontend
/// arguments.
function retrieveWsArgs() {
  /// The string `preview-arg:previewMode:Doc` is a placeholder
  /// It will be replaced by the actual preview mode.
  /// ```rs
  ///   let frontend_html = frontend_html.replace(
  ///     "preview-arg:previewMode:Doc", ...);
  /// ```
  let mode = "preview-arg:previewMode:Doc";
  /// Remove the placeholder prefix.
  mode = mode.replace("preview-arg:previewMode:", "");
  let previewMode = PreviewMode[mode];

  /// The string `ws://127.0.0.1:23625` is a placeholder
  /// Also, it is the default url to connect to.
  /// Note that we must resolve the url to an absolute url as
  /// the websocket connection requires an absolute url.
  ///
  /// See [WebSocket and relative URLs](https://github.com/whatwg/websockets/issues/20)
  let urlObject = new URL("ws://127.0.0.1:23625", window.location.href);
  /// Rewrite the protocol to websocket.
  urlObject.protocol = urlObject.protocol.replace("https:", "wss:").replace("http:", "ws:");
  if (location.href.startsWith("https://")) {
    urlObject.protocol = urlObject.protocol.replace("ws:", "wss:");
  }

  /// Return a `WsArgs` object.
  const nativeTheme = "preview-arg:systemTheme:false".endsWith(":true");
  return { url: urlObject.href, previewMode, isContentPreview: false, nativeTheme };
}

/// `buildWs` returns a object, which keeps track of websocket
///  connections.
function buildWs() {
  /// `nextWs` will always hold a global unique websocket connection
  /// to the preview backend.
  const nextWs = createConnectionQueue(wsMain, (args) => {
    const previousDoc = windowElem.documents[0];
    const readingState =
      args.nativeTheme && previousDoc
        ? captureReadingState(document.getElementById("typst-container-main"), previousDoc.impl)
        : undefined;
    resetAppMode(args);
    return { ...args, readingState };
  });

  return { nextWs };

  function resetAppMode({ previewMode: mode, isContentPreview }) {
    const app = document.getElementById("typst-container");
    const helpPanel = document.getElementById("typst-help-panel");

    /// Set the root css selector to the content preview mode.
    app.classList.remove("content-preview");
    if (isContentPreview) {
      app.classList.add("content-preview");
    }

    /// Set the root css selector to the preview mode.
    app.classList.remove("mode-slide");
    app.classList.remove("mode-doc");
    helpPanel?.classList.remove("mode-slide");
    helpPanel?.classList.remove("mode-doc");
    if (mode === PreviewMode.Slide) {
      app.classList.add("mode-slide");
      helpPanel?.classList.add("mode-slide");
    } else if (mode === PreviewMode.Doc) {
      app.classList.add("mode-doc");
      helpPanel?.classList.add("mode-doc");
    } else {
      throw new Error(`Unknown preview mode: ${mode}`);
    }
  }
}

/// A frontend will try to setup a vscode channel if it is running
/// in vscode.
function setupVscodeChannel(nextWs) {
  const vscodeAPI = typeof acquireVsCodeApi !== "undefined" && acquireVsCodeApi();
  if (vscodeAPI?.postMessage) {
    vscodeAPI.postMessage({ type: "started" });
  }
  if (vscodeAPI?.setState && window.vscode_state) {
    vscodeAPI.setState(window.vscode_state);
  }

  // Handle messages sent from the extension to the webview
  window.addEventListener("message", (event) => {
    const message = event.data; // The json data that the extension sent
    switch (message.type) {
      case "reconnect": {
        console.log("reconnect", message);
        nextWs({
          url: message.url,
          previewMode: PreviewMode[message.mode],
          isContentPreview: message.isContentPreview,
        });
        break;
      }
      case "outline": {
        console.log("outline", message);
        break;
      }
    }
  });
}
