import { PreviewMode } from "typst-dom/typst-doc.mjs";
import { hasTouchSelection } from "typst-dom/touch-selection.mjs";
import {
  TypstPreviewDocument as TypstDocument,
  TypstDomHookedElement,
  TypstDomWindowElement,
} from "typst-dom/index.preview.mjs";
import {
  rendererBuildInfo,
  createTypstRenderer,
} from "@myriaddreamin/typst.ts/dist/esm/renderer.mjs";
import renderModule from "@myriaddreamin/typst-ts-renderer/pkg/typst_ts_renderer_bg.wasm?url";
// @ts-ignore
// import { RenderSession as RenderSession2 } from "@myriaddreamin/typst-ts-renderer/pkg/wasm-pack-shim.mjs";
import { RenderSession } from "@myriaddreamin/typst.ts/dist/esm/renderer.mjs";
import { WebSocketSubject, webSocket } from "rxjs/webSocket";
import { Subject, Subscription, buffer, debounceTime, auditTime, fromEvent, tap } from "rxjs";
import { handleHtmlPreviewFrame } from "./html-preview";
import type { ReadingState } from "./document-theme";
import { ChangeLocationQueue, parseChangeLocation, type ChangeLocation } from "./change-location";
import { focusStatusPlacement } from "./mobile-viewport";
export { PreviewMode } from "typst-dom/typst-doc.mjs";

// for debug propose
// queryObjects((window as any).TypstRenderSession);
(window as any).TypstRenderSession = RenderSession;
// (window as any).TypstRenderSessionKernel = RenderSession2;

const enc = new TextEncoder();
const dec = new TextDecoder();
const NOT_AVAILABLE = "current not available";
const COMMA = enc.encode(",");
export interface WsArgs {
  url: string;
  previewMode: PreviewMode;
  isContentPreview: boolean;
  nativeTheme?: boolean;
  onToggleTheme?: () => void;
  readingState?: ReadingState;
}

export async function wsMain({
  url,
  previewMode,
  isContentPreview,
  nativeTheme,
  onToggleTheme,
  readingState,
}: WsArgs) {
  if (!url) {
    const hookedElem = document.getElementById("typst-app");
    if (hookedElem) {
      hookedElem.innerHTML = "";
    }
    return () => {};
  }
  const windowElem = document.getElementById("typst-container")! as TypstDomWindowElement;

  let disposed = false;
  let $ws: WebSocketSubject<ArrayBuffer> | undefined = undefined;
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  const subsribes: Subscription[] = [];
  let queueChangedLocation: ((location: ChangeLocation, resume: boolean) => void) | undefined;
  let clearChangedLocation: (() => void) | undefined;
  let focusRevision: string | undefined;
  let pendingFocusRevision: string | undefined;
  let focusStatus: HTMLDivElement | undefined;
  const positionFocusStatus = () => {
    const viewport = window.visualViewport;
    if (!focusStatus || !viewport) return;
    const style = getComputedStyle(document.documentElement);
    const inset = (edge: string) =>
      Number.parseFloat(style.getPropertyValue(`--preview-safe-${edge}`)) || 0;
    Object.assign(
      focusStatus.style,
      focusStatusPlacement(viewport, {
        left: inset("left"),
        right: inset("right"),
        bottom: inset("bottom"),
      }),
    );
  };
  const korean = navigator.language.startsWith("ko");
  const showFocusStatus = (ko: string, en: string) => {
    if (!focusStatus) {
      focusStatus = document.createElement("div");
      focusStatus.className = "typst-focus-status";
      focusStatus.setAttribute("role", "status");
      focusStatus.setAttribute("aria-live", "polite");
      document.body.appendChild(focusStatus);
    }
    focusStatus.textContent = korean ? ko : en;
    positionFocusStatus();
  };

  function createSvgDocument(kModule: RenderSession) {
    const hookedElem = document.getElementById("typst-app")! as TypstDomHookedElement;
    if (hookedElem.firstElementChild?.tagName !== "svg") {
      hookedElem.innerHTML = "";
    }
    const resizeTarget = document.getElementById("typst-container-main")!;

    const svgDoc = new TypstDocument({
      windowElem,
      hookedElem,
      kModule,
      previewMode,
      isContentPreview,
      // set rescale target to `body`
      retrieveDOMState() {
        return {
          width: resizeTarget.clientWidth,
          height: resizeTarget.offsetHeight,
          boundingRect: resizeTarget.getBoundingClientRect(),
        };
      },
    });
    if (readingState) {
      svgDoc.impl.currentScaleRatio = readingState.scale;
      svgDoc.impl.partialRenderPage = readingState.page;
    }

    if (previewMode === PreviewMode.Doc && !isContentPreview) {
      const changedLocations = new ChangeLocationQueue(Boolean(readingState));
      let lastUserGesture = -Infinity;
      const changeButton = document.createElement("button");
      changeButton.type = "button";
      changeButton.className = "typst-change-jump";
      changeButton.textContent = navigator.language.startsWith("ko")
        ? "변경 위치로"
        : "Jump to change";
      changeButton.hidden = true;
      document.body.appendChild(changeButton);

      const jumpToChange = ([page, , y]: [number, number, number]) => {
        const rect = hookedElem.querySelector<SVGRectElement>(
          `.typst-page-inner[data-page-number="${page - 1}"]`,
        );
        if (!rect) return;
        const pageHeight = Number.parseFloat(rect.getAttribute("data-page-height") || "0");
        const renderedRect = rect.getBoundingClientRect();
        const pageTop =
          renderedRect.top - resizeTarget.getBoundingClientRect().top + resizeTarget.scrollTop;
        const innerY =
          pageHeight > 0
            ? (Math.min(Math.max(y, 0), pageHeight) * renderedRect.height) / pageHeight
            : 0;
        // An instant jump avoids animating through hundreds of virtual pages.
        resizeTarget.scrollTo({
          top: Math.max(0, pageTop + innerY - resizeTarget.clientHeight * 0.35),
          behavior: "instant",
        });
        svgDoc.addViewportChange();
      };

      const showChangeButton = () => {
        changeButton.hidden = false;
      };
      changeButton.addEventListener("click", () => {
        if (changedLocations.lastLocation) jumpToChange(changedLocations.lastLocation);
        changeButton.hidden = true;
      });

      const markUserGesture = () => {
        lastUserGesture = performance.now();
      };
      resizeTarget.addEventListener("pointerdown", markUserGesture, { passive: true });
      resizeTarget.addEventListener("touchmove", markUserGesture, { passive: true });
      resizeTarget.addEventListener("wheel", markUserGesture, { passive: true });
      document.addEventListener("keydown", markUserGesture);

      queueChangedLocation = (location, resume) => {
        changedLocations.queue(location, resume);
      };
      clearChangedLocation = () => {
        changedLocations.clear();
        changeButton.hidden = true;
      };
      svgDoc.impl.onDidRender = () => {
        const change = changedLocations.afterRender(performance.now(), lastUserGesture);
        if (!change) return;
        if (change.deferred) {
          showChangeButton();
        } else {
          jumpToChange(change.location);
          changeButton.hidden = true;
        }
      };
      svgDoc.impl.disposeList.push(() => {
        changeButton.remove();
        resizeTarget.removeEventListener("pointerdown", markUserGesture);
        resizeTarget.removeEventListener("touchmove", markUserGesture);
        resizeTarget.removeEventListener("wheel", markUserGesture);
        document.removeEventListener("keydown", markUserGesture);
        queueChangedLocation = undefined;
        clearChangedLocation = undefined;
      });
    }

    const previousOnDidRender = svgDoc.impl.onDidRender;
    let restoreReadingState = readingState;
    svgDoc.impl.onDidRender = () => {
      focusRevision = pendingFocusRevision;
      previousOnDidRender?.();
      if (restoreReadingState) {
        const state = restoreReadingState;
        restoreReadingState = undefined;
        resizeTarget.scrollTo({ top: state.top, left: state.left, behavior: "instant" });
        svgDoc.addViewportChange();
      }
    };
    svgDoc.impl.disposeList.push(() => {
      windowElem.onPreviewFocus = undefined;
      focusStatus?.remove();
      focusStatus = undefined;
    });

    // Dynamic viewport units can settle after the window resize event on iOS.
    // Observe the actual scrollport; the renderer retains its existing resize anchor
    // and scale ratio. Do not resize the document to the pinch-zoomed visual viewport.
    if (typeof ResizeObserver !== "undefined") {
      const observer = new ResizeObserver(() => {
        positionFocusStatus();
        svgDoc.addViewportChange();
      });
      observer.observe(resizeTarget);
      svgDoc.impl.disposeList.push(() => observer.disconnect());
    }
    subsribes.push(fromEvent(window, "resize").subscribe(() => svgDoc.addViewportChange()));
    if (window.visualViewport) {
      for (const event of ["resize", "scroll"]) {
        subsribes.push(
          fromEvent(window.visualViewport, event)
            .pipe(auditTime(80))
            .subscribe(() => {
              positionFocusStatus();
              svgDoc.addViewportChange();
            }),
        );
      }
    }

    if (!isContentPreview) {
      subsribes.push(
        fromEvent(resizeTarget, "scroll", { passive: true })
          .pipe(auditTime(80))
          .subscribe(() => svgDoc.addViewportChange()),
      );
    }

    // Handle messages sent from the extension to the webview
    subsribes.push(
      fromEvent<MessageEvent>(window, "message").subscribe((event) => {
        const message = event.data; // The json data that the extension sent
        switch (message.type) {
          case "outline": {
            svgDoc.setOutineData(message.outline);
            break;
          }
        }
      }),
    );

    const focusInput = () => {
      const inpPageSelector = document.getElementById("typst-page-selector") as
        | HTMLSelectElement
        | undefined;
      if (inpPageSelector) {
        inpPageSelector.focus();
      }
    };

    const blurInput = () => {
      const inpPageSelector = document.getElementById("typst-page-selector") as
        | HTMLSelectElement
        | undefined;
      if (inpPageSelector) {
        inpPageSelector.blur();
      }
    };

    const updateDiff = (diff: number) => () => {
      const pageSelector = document.getElementById("typst-page-selector") as
        | HTMLSelectElement
        | undefined;

      if (pageSelector) {
        console.log("updateDiff", diff);
        const v = pageSelector.value;
        if (v.length === 0) {
          return;
        }
        const page = Number.parseInt(v) + diff;
        if (page <= 0) {
          return;
        }
        if (svgDoc.setPartialPageNumber(page)) {
          pageSelector.value = page.toString();
          blurInput();
        }
      }
    };

    const updatePrev = updateDiff(-1);
    const updateNext = updateDiff(1);

    const pagePrevSelector = document.getElementById("typst-page-prev-selector");
    if (pagePrevSelector) {
      pagePrevSelector.addEventListener("click", updatePrev);
    }
    const pageNextSelector = document.getElementById("typst-page-next-selector");
    if (pageNextSelector) {
      pageNextSelector.addEventListener("click", updateNext);
    }
    svgDoc.impl.disposeList.push(() => {
      pagePrevSelector?.removeEventListener("click", updatePrev);
      pageNextSelector?.removeEventListener("click", updateNext);
    });

    if (previewMode === PreviewMode.Slide) {
      {
        const inpPageSelector = document.getElementById("typst-page-selector") as
          | HTMLSelectElement
          | undefined;
        if (inpPageSelector) {
          const changePage = () => {
            if (inpPageSelector.value.length === 0) {
              return;
            }
            const page = Number.parseInt(inpPageSelector.value);
            svgDoc.setPartialPageNumber(page);
          };
          inpPageSelector.addEventListener("input", changePage);
          svgDoc.impl.disposeList.push(() =>
            inpPageSelector.removeEventListener("input", changePage),
          );
        }
      }
    }

    const toggleHelp = () => {
      const help = document.getElementById("typst-help-panel");
      console.log("toggleHelp", help);
      if (help) {
        help.classList.toggle("hidden");
      }
    };

    const removeHelp = () => {
      const help = document.getElementById("typst-help-panel");
      if (help) {
        help.classList.add("hidden");
      }
    };

    const toggleTheme = () => {
      if (nativeTheme) {
        onToggleTheme?.();
        return;
      }
      const typstApp = document.getElementById("typst-app");
      console.log("toggleTheme", typstApp);
      if (typstApp) {
        typstApp.classList.toggle("invert-colors");
      }
    };

    const helpButton = document.getElementById("typst-top-help-button");
    helpButton?.addEventListener("click", toggleHelp);

    const handleKeyDown = (e: KeyboardEvent) => {
      if (
        e.target instanceof HTMLElement &&
        e.target.matches("input, select, textarea, [contenteditable]")
      )
        return;
      let handled = true;

      const scrollDelta = 50;

      switch (e.key) {
        case "ArrowLeft":
        case "ArrowUp":
          if (previewMode === PreviewMode.Slide) {
            blurInput();
            removeHelp();
            updatePrev();
          }
          break;
        case " ":
        case "ArrowRight":
        case "ArrowDown":
          if (previewMode === PreviewMode.Slide) {
            blurInput();
            removeHelp();
            updateNext();
          }
          break;
        case "j":
          resizeTarget.scrollBy({ top: +scrollDelta, behavior: "instant" });
          break;
        case "k":
          resizeTarget.scrollBy({ top: -scrollDelta, behavior: "instant" });
          break;
        case "h":
          resizeTarget.scrollBy({ top: -scrollDelta * 10, behavior: "smooth" });
          break;
        case "l":
          resizeTarget.scrollBy({ top: +scrollDelta * 10, behavior: "smooth" });
          break;
        case "?":
          blurInput();
          toggleHelp();
          break;
        case "g":
          removeHelp();
          focusInput();
          break;
        case "Escape":
          removeHelp();
          blurInput();
          handled = false;
          break;
        case "t":
          toggleTheme();
          break;
        default:
          handled = false;
      }

      if (handled) {
        e.preventDefault();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    svgDoc.impl.disposeList.push(() => {
      window.removeEventListener("keydown", handleKeyDown);
      helpButton?.removeEventListener("click", toggleHelp);
    });

    return svgDoc;
  }

  function setupSocket(svgDoc: TypstDocument): () => void {
    if (!windowElem.documents.includes(svgDoc)) windowElem.documents.push(svgDoc);

    // todo: reconnect setTimeout(() => setupSocket(svgDoc), 1000);
    $ws = webSocket<ArrayBuffer>({
      url,
      binaryType: "arraybuffer",
      serializer: (t) => t,
      deserializer: (event) => event.data,
      openObserver: {
        next: (e) => {
          if (disposed) return;
          const sock = e.target;
          console.log("WebSocket connection opened", sock);
          windowElem.typstWebsocket = sock as any;
          svgDoc.reset();
          clearChangedLocation?.();
          focusRevision = pendingFocusRevision = undefined;
          windowElem.onPreviewFocus = undefined;
          windowElem.typstWebsocket.send("current");
        },
      },
      closeObserver: {
        next: (e) => {
          console.log("WebSocket connection closed", e);
          focusRevision = pendingFocusRevision = undefined;
          if (focusStatus)
            showFocusStatus(
              "연결 끊김 · 다시 연결한 뒤 탭해 주세요",
              "Disconnected · reconnect, then tap again",
            );
          $ws?.unsubscribe();
          if (!disposed) {
            reconnectTimer = setTimeout(() => {
              if (!disposed) setupSocket(svgDoc);
            }, 1000);
          }
        },
      },
    });

    const batchMessageChannel = new Subject<ArrayBuffer>();

    const dispose = () => {
      disposed = true;
      if (reconnectTimer !== undefined) clearTimeout(reconnectTimer);
      svgDoc.dispose();
      const index = windowElem.documents.indexOf(svgDoc);
      if (index >= 0) {
        windowElem.documents.splice(index, 1);
      }
      for (const sub of subsribes.splice(0, subsribes.length)) {
        sub.unsubscribe();
      }
      $ws?.complete();
      windowElem.typstWebsocket = undefined as any;
    };

    // window.typstWebsocket = new WebSocket("ws://127.0.0.1:23625");

    $ws.subscribe({
      next: (data) => batchMessageChannel.next(data), // Called whenever there is a message from the server.
      error: (err) => console.log("WebSocket Error: ", err), // Called if at any point WebSocket API signals some kind of error.
      complete: () => console.log("complete"), // Called when connection is closed (for whatever reason).
    });

    subsribes.push(
      batchMessageChannel
        .pipe(buffer(batchMessageChannel.pipe(debounceTime(0))))
        .pipe(
          tap((dataList) => {
            console.log(`batch ${dataList.length} messages`);
          }),
        )
        .subscribe((dataList) => {
          dataList.map(processMessage);
        }),
    );

    function processMessage(data: ArrayBuffer) {
      if (!(data instanceof ArrayBuffer)) {
        if (data === NOT_AVAILABLE) {
          return;
        }

        console.error("WebSocket data is not a ArrayBuffer", data);
        return;
      }

      const buffer = data;
      const messageData = new Uint8Array(buffer);

      const message_idx = messageData.indexOf(COMMA[0]);
      const message = [
        dec.decode(messageData.slice(0, message_idx).buffer),
        messageData.slice(message_idx + 1),
      ];
      console.log("recv", message[0], messageData.length);
      if (message[0] === "focus-enabled") {
        showFocusStatus(
          "Codex에 위치 공유 · 본문을 탭하세요",
          "Share a location with Codex · tap the document",
        );
        windowElem.onPreviewFocus = (position) => {
          if (hasTouchSelection(svgDoc.impl.hookedElem)) return;
          if (!focusRevision || svgDoc.impl.isRendering || svgDoc.impl.patchQueue.length) {
            showFocusStatus(
              "문서 갱신 중 · 잠시 후 다시 탭해 주세요",
              "Document updating · tap again shortly",
            );
            return;
          }
          showFocusStatus("선택한 위치 저장 중…", "Saving selected location…");
          windowElem.typstWebsocket.send(
            `src-point ${JSON.stringify({ ...position, revision: focusRevision })}`,
          );
        };
        return;
      }
      if (message[0] === "focus-revision") {
        pendingFocusRevision = dec.decode(message[1] as Uint8Array);
        focusRevision = undefined;
        return;
      }
      if (message[0] === "focus") {
        const result = JSON.parse(dec.decode(message[1] as Uint8Array));
        if (result.status === "selected") {
          const name = result.filepath.split(/[\\/]/).pop();
          showFocusStatus(
            `Codex용 위치 저장됨 · ${result.page}쪽 · ${name}:${result.line}`,
            `Saved for Codex · p. ${result.page} · ${name}:${result.line}`,
          );
        } else if (result.status === "unmapped") {
          showFocusStatus(
            `${result.page}쪽 위치만 저장됨 · 글자를 탭하면 소스도 연결됩니다`,
            `Page ${result.page} location saved · tap text to link its source`,
          );
        } else if (result.status === "stale" || result.status === "not_ready") {
          showFocusStatus("문서가 갱신됐습니다 · 다시 탭해 주세요", "Document changed · tap again");
        } else {
          showFocusStatus(
            "위치 저장 실패 · 다시 탭해 주세요",
            "Could not save location · tap again",
          );
        }
        return;
      }
      // console.log(message[0], message[1].length);
      if (isContentPreview) {
        // whether to scroll to the content preview when user updates document
        const autoScrollContentPreview = true;
        if (!autoScrollContentPreview && message[0] === "jump") {
          return;
        }

        // "viewport": viewport change to document doesn't affect content preview
        // "partial-rendering": content previe always render partially
        // "cursor": currently not supported
        if (
          message[0] === "viewport" ||
          message[0] === "partial-rendering" ||
          message[0] === "cursor"
        ) {
          return;
        }
      }

      if (message[0] === "change" || message[0] === "resume") {
        const location = parseChangeLocation(dec.decode(message[1] as Uint8Array));
        if (location) queueChangedLocation?.(location, message[0] === "resume");
        return;
      }

      if (message[0] === "jump" || message[0] === "viewport") {
        const rootElem = document.getElementById("typst-app")?.firstElementChild;

        // todo: aware height padding
        let currentPageNumber = 1;
        if (previewMode === PreviewMode.Slide) {
          currentPageNumber = svgDoc.getPartialPageNumber();
        } else if (rootElem) {
          currentPageNumber = windowElem.currentPosition(rootElem)?.page || 1;
        }

        let positions = dec
          .decode((message[1] as any).buffer)
          .split(",")
          .map((t: string) => t.trim())
          .filter((t: string) => t.length > 0);

        // choose the page, x, y closest to the current page
        const [page, x, y] = positions.reduce(
          (acc, cur) => {
            const [page, x, y] = cur.split(" ").map(Number);
            const current_page = currentPageNumber;
            // If page distance is the same, choose the last one
            if (Math.abs(page - current_page) <= Math.abs(acc[0] - current_page)) {
              return [page, x, y];
            }
            return acc;
          },
          [Number.MAX_SAFE_INTEGER, 0, 0],
        );
        // console.log("resolved", page, x, y, "from", currentPageNumber);

        let pageToJump = page;
        if (pageToJump === Number.MAX_SAFE_INTEGER) {
          return;
        }

        if (previewMode === PreviewMode.Slide) {
          const pageSelector = document.getElementById("typst-page-selector") as
            | HTMLSelectElement
            | undefined;
          if (svgDoc.setPartialPageNumber(page)) {
            if (pageSelector) {
              pageSelector.value = page.toString();
            }
            // pageToJump = 1;
            // todo: hint location
            return;
          } else {
            return;
          }
        }

        if (rootElem) {
          /// Note: when it is really scrolled, it will trigger `svgDoc.addViewportChange`
          /// via `window.onscroll` event
          windowElem.handleTypstLocation(rootElem, pageToJump, x, y);
        }
        return;
      } else if (message[0] === "cursor") {
        // todo: aware height padding
        const [page, x, y] = dec
          .decode((message[1] as any).buffer)
          .split(" ")
          .map(Number);
        console.log("cursor", page, x, y);
        svgDoc.setCursor(page, x, y);
        svgDoc.addViewportChange(); // todo: synthesizing cursor event
        return;
      } else if (message[0] === "cursor-paths") {
        // todo: aware height padding
        const paths = JSON.parse(dec.decode((message[1] as any).buffer));
        console.log("cursor-paths", paths);
        svgDoc.impl.setCursorPaths(paths);
        return;
      } else if (message[0] === "partial-rendering") {
        console.log("Experimental feature: partial rendering enabled");
        svgDoc.setPartialRendering(true);
        return;
      } else if (message[0] === "invert-colors") {
        if (nativeTheme) {
          document.getElementById("typst-app")?.classList.remove("invert-colors", "normal-image");
          return;
        }
        const rawStrategy = dec.decode((message[1] as any).buffer).trim();
        const strategy =
          INVERT_COLORS_STRATEGY.find((t) => t === rawStrategy) ||
          (JSON.parse(rawStrategy) as StrategyMap);
        console.log("Experimental feature: invert colors strategy taken:", strategy);
        ensureInvertColors(document.getElementById("typst-app"), strategy);
        return;
      } else if (message[0] === "outline") {
        console.log("Experimental feature: outline rendering");
        return;
      }

      if (handleHtmlPreviewFrame(message[0] as string, message[1] as Uint8Array, url, dec)) {
        return;
      }

      svgDoc.addChangement(message as any);
    }

    return dispose;
  }

  let plugin = createTypstRenderer();
  await plugin.init({ getModule: () => renderModule });

  return new Promise<() => void>((resolveDispose) =>
    plugin.runWithSession((kModule) /* module kernel from wasm */ => {
      return new Promise(async (kernelDispose) => {
        console.log("plugin initialized, build info:", await rendererBuildInfo());

        const wsDispose = setupSocket(createSvgDocument(kModule));

        // todo: plugin init and setup socket at the same time
        resolveDispose(() => {
          // dispose ws first
          wsDispose();
          // dispose kernel then
          kernelDispose(undefined);
        });
      });
    }),
  );
}

/** The strategy to set invert colors, see editors/vscode/package.json for enum descriptions */
const INVERT_COLORS_STRATEGY = ["never", "auto", "always"] as const;
/** The value of strategy constant */
type StrategyKey = (typeof INVERT_COLORS_STRATEGY)[number];
/** The map from element kinds to strategy */
type StrategyMap = Partial<Record<"rest" | "image", StrategyKey>>;

function ensureInvertColors(root: HTMLElement | null, strategy: StrategyKey | StrategyMap) {
  if (!root) {
    return;
  }

  // Uniforms type of strategy to `TargetMap`
  if (typeof strategy === "string") {
    strategy = { rest: strategy };
  }

  let autoDecision: { value: boolean } | undefined = undefined;
  /**
   * Handles invert colors mode based on a string enumerated strategy.
   * @param strategy - The strategy set by user.
   * @returns needInvertColor - Use or not use invert color.
   */
  const decide = (strategy: StrategyKey) => {
    switch (strategy) {
      case "never":
        return false;
      default:
        console.warn("Unknown invert-colors strategy:", strategy);
        return false;
      case "auto":
        return (autoDecision ||= { value: determineInvertColor() }).value;
      case "always":
        return true;
    }
  };

  root.classList.toggle("invert-colors", decide(strategy?.rest || "never"));
  root.classList.toggle("normal-image", !decide(strategy?.image || strategy?.rest || "never"));

  function determineInvertColor() {
    const vscodeAPI = typeof acquireVsCodeApi !== "undefined";

    if (vscodeAPI) {
      // vscode-dark, high-contrast, vscode-light
      const cls = document.body.classList;
      const themeIsDark =
        (cls.contains("vscode-dark") || cls.contains("vscode-high-contrast")) &&
        !cls.contains("vscode-light");

      if (themeIsDark) {
        console.log("invert-colors because detected by vscode theme", document.body.className);
        return true;
      }
    } else {
      // prefer dark mode
      if (window.matchMedia("(prefers-color-scheme: dark)").matches) {
        console.log("invert-colors because detected by (prefers-color-scheme: dark)");
        return true;
      }
    }

    console.log("doesn't invert-colors because none of dark mode detected");
    return false;
  }
}
