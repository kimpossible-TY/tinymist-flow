import { focusStatusPlacement } from "./mobile-viewport";
import { ReviewMarks } from "./review-marks";
import { previewPages, selectedPreviewText, type SelectedText } from "./text-selection";

export class SelectionActions {
  private toolbar = document.createElement("div");
  private layer = document.createElement("div");
  private selection?: SelectedText;
  private pressedSelection?: SelectedText;
  private pressing = false;
  private releaseTimer?: ReturnType<typeof setTimeout>;
  private marks: ReviewMarks;
  private saveButton: HTMLButtonElement;
  private highlightButton: HTMLButtonElement;
  private strikeButton: HTMLButtonElement;
  private undoButton: HTMLButtonElement;
  private clearButton: HTMLButtonElement;
  private enabled = false;
  private disposed = false;
  private frame?: number;
  private korean = navigator.language.startsWith("ko");

  constructor(
    private root: HTMLElement,
    private onAction: (action: "save" | "highlight", selection: SelectedText) => boolean,
    private readRevision: () => string | undefined,
  ) {
    let storage: Storage | undefined;
    try {
      storage = window.localStorage;
    } catch {
      /* Fall back to memory. */
    }
    this.marks = new ReviewMarks(storage);
    this.toolbar.className = "typst-selection-actions";
    this.toolbar.setAttribute("role", "toolbar");
    this.toolbar.setAttribute("aria-label", this.korean ? "선택한 텍스트" : "Selected text");
    this.toolbar.hidden = true;
    this.layer.className = "typst-review-marks";
    this.layer.setAttribute("aria-hidden", "true");
    root.classList.add("typst-review-host");
    document.body.appendChild(this.toolbar);
    const button = (ko: string, en: string, action: (selected?: SelectedText) => void) => {
      const element = document.createElement("button");
      element.type = "button";
      element.textContent = this.korean ? ko : en;
      element.addEventListener("pointerdown", (event) => {
        this.pressing = true;
        this.pressedSelection = this.selection;
        event.preventDefault();
      });
      element.addEventListener("mousedown", (event) => event.preventDefault());
      const release = () => {
        if (this.releaseTimer !== undefined) clearTimeout(this.releaseTimer);
        this.releaseTimer = setTimeout(() => {
          this.pressing = false;
          this.pressedSelection = undefined;
          this.updateButtons();
        }, 0);
      };
      element.addEventListener("pointerup", release);
      element.addEventListener("pointercancel", release);
      element.addEventListener("click", () => {
        const selected = this.pressedSelection ?? this.selection;
        this.pressing = false;
        this.pressedSelection = undefined;
        action(selected);
        this.updateButtons();
      });
      this.toolbar.appendChild(element);
      return element;
    };
    this.highlightButton = button("하이라이트", "Highlight", (selected) => {
      if (selected && this.onAction("highlight", selected)) this.collapse();
    });
    this.saveButton = button("Codex 저장", "Save for Codex", (selected) => {
      if (selected) this.onAction("save", selected);
    });
    this.strikeButton = button("빨간 취소선", "Red strike", (selected) => {
      if (!selected) return;
      this.marks.add(selected);
      this.collapse();
      this.refresh();
    });
    this.strikeButton.className = "typst-strike-action";
    this.undoButton = button("되돌리기", "Undo", () => {
      this.marks.undo();
      this.refresh();
    });
    this.clearButton = button("표시 지우기", "Clear marks", () => {
      this.marks.clear();
      this.refresh();
    });
    document.addEventListener("selectionchange", this.selectionChanged);
    root.addEventListener("typst-text-layout", this.refresh);
    window.addEventListener("resize", this.position);
    window.visualViewport?.addEventListener("resize", this.position);
    window.visualViewport?.addEventListener("scroll", this.position);
    this.selectionChanged();
  }

  setEnabled(enabled: boolean) {
    this.enabled = enabled;
    this.updateButtons();
  }
  setDocument(id: string) {
    this.marks.setDocument(id);
    this.afterRender();
  }
  afterRender() {
    if (this.frame !== undefined) cancelAnimationFrame(this.frame);
    this.frame = requestAnimationFrame(() => {
      this.frame = undefined;
      if (!this.disposed) this.refresh();
    });
  }

  private selectionChanged = () => {
    this.selection = selectedPreviewText(this.root);
    if (this.selection) this.selection.revision = this.readRevision();
    this.updateButtons();
  };

  private collapse() {
    document.getSelection()?.removeAllRanges();
    this.selection = undefined;
    this.updateButtons();
  }

  private updateButtons() {
    if (this.pressing) return;
    const selected = !!this.selection;
    this.highlightButton.hidden = this.saveButton.hidden = this.strikeButton.hidden = !selected;
    const ready =
      this.enabled && !!this.selection?.revision && this.selection.revision === this.readRevision();
    this.saveButton.disabled = this.highlightButton.disabled = !ready;
    this.undoButton.hidden = this.clearButton.hidden = !this.marks.marks.length;
    this.toolbar.hidden = !selected && !this.marks.marks.length;
    this.position();
  }

  position = () => {
    const viewport = window.visualViewport;
    if (!viewport) return;
    const style = getComputedStyle(document.documentElement);
    const inset = (edge: string) =>
      Number.parseFloat(style.getPropertyValue(`--preview-safe-${edge}`)) || 0;
    const placement = focusStatusPlacement(viewport, {
      left: inset("left"),
      right: inset("right"),
      bottom: inset("bottom"),
    });
    // Leave room for status messages even when their text wraps onto more lines.
    const statusHeight =
      document.querySelector(".typst-focus-status")?.getBoundingClientRect().height || 0;
    placement.top = `${Number.parseFloat(placement.top) - statusHeight - 8 / (viewport.scale || 1)}px`;
    Object.assign(this.toolbar.style, placement);
  };

  private refresh = () => {
    if (this.disposed) return;
    // The renderer owns the first child and initializes an empty container.
    if (this.root.firstElementChild?.tagName !== "svg") return;
    if (!this.layer.isConnected) this.root.appendChild(this.layer);
    const pages = previewPages(this.root);
    this.marks.reconcile(pages);
    const host = this.root.getBoundingClientRect();
    const lines: HTMLElement[] = [];
    for (const mark of this.marks.marks)
      for (const fragment of mark.fragments) {
        const page = pages.get(fragment.page);
        if (!page?.fingerprint) continue;
        const line = document.createElement("span");
        const sx = page.rect.width / page.width,
          sy = page.rect.height / page.height;
        Object.assign(line.style, {
          left: `${page.rect.left - host.left + fragment.x * sx}px`,
          top: `${page.rect.top - host.top + (fragment.y + fragment.height * 0.5) * sy}px`,
          width: `${fragment.width * sx}px`,
        });
        lines.push(line);
      }
    this.layer.replaceChildren(...lines);
    this.updateButtons();
  };

  dispose() {
    this.disposed = true;
    if (this.releaseTimer !== undefined) clearTimeout(this.releaseTimer);
    if (this.frame !== undefined) cancelAnimationFrame(this.frame);
    document.removeEventListener("selectionchange", this.selectionChanged);
    this.root.removeEventListener("typst-text-layout", this.refresh);
    window.removeEventListener("resize", this.position);
    window.visualViewport?.removeEventListener("resize", this.position);
    window.visualViewport?.removeEventListener("scroll", this.position);
    this.toolbar.remove();
    this.layer.remove();
    this.root.classList.remove("typst-review-host");
  }
}
