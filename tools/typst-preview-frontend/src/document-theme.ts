export type DocumentTheme = "light" | "dark";
export type ThemePreference = "system" | DocumentTheme;

/** Owns device listening and defers document replacement during native selection. */
export class DocumentThemeController {
  preference: ThemePreference = "system";
  private applied?: DocumentTheme;
  private disposed = false;
  private refresh = () => {
    if (this.disposed || this.hasSelection()) return;
    const theme =
      this.preference === "system" ? (this.media.matches ? "dark" : "light") : this.preference;
    if (theme === this.applied) return;
    this.applied = theme;
    this.onTheme(theme);
  };

  constructor(
    private media: Pick<MediaQueryList, "matches" | "addEventListener" | "removeEventListener">,
    private selectionEvents: Pick<Document, "addEventListener" | "removeEventListener">,
    private hasSelection: () => boolean,
    private onTheme: (theme: DocumentTheme) => void,
  ) {
    media.addEventListener("change", this.refresh);
    selectionEvents.addEventListener("selectionchange", this.refresh);
    this.refresh();
  }

  setPreference(preference: ThemePreference) {
    this.preference = preference;
    this.refresh();
  }

  dispose() {
    this.disposed = true;
    this.media.removeEventListener("change", this.refresh);
    this.selectionEvents.removeEventListener("selectionchange", this.refresh);
  }
}

export function themeWebSocketUrl(url: string, theme: DocumentTheme): string {
  const result = new URL(url);
  result.pathname = `/_theme/${theme}`;
  return result.href;
}

export interface ReadingState {
  top: number;
  left: number;
  scale: number;
  page: number;
}

export function captureReadingState(
  container: Pick<HTMLElement, "scrollTop" | "scrollLeft">,
  context: { currentScaleRatio: number; partialRenderPage: number },
): ReadingState {
  return {
    top: container.scrollTop,
    left: container.scrollLeft,
    scale: context.currentScaleRatio,
    page: context.partialRenderPage,
  };
}

/** Serialize transitions and skip obsolete requests before allocating a renderer. */
export function createConnectionQueue<T>(
  connect: (args: T) => Promise<() => void>,
  beforeConnect: (args: T) => T,
) {
  let generation = 0;
  let dispose = () => {};
  let queue = Promise.resolve();
  return (args: T) => {
    const current = ++generation;
    queue = queue.catch(console.error).then(async () => {
      if (current !== generation) return;
      const next = beforeConnect(args);
      dispose();
      dispose = () => {};
      dispose = await connect(next);
    });
    return queue;
  };
}
