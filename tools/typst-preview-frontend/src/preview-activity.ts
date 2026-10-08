export type ActivityPhase =
  | "preparing"
  | "connecting"
  | "waiting"
  | "compiling"
  | "rendering"
  | "selection-held"
  | "reconnecting"
  | "compile-error"
  | "render-error"
  | "ready";

/** Transport and compiler completion must not masquerade as a rendered document. */
export class PreviewActivityState {
  private connection: "preparing" | "connecting" | "connected" | "reconnecting" = "preparing";
  private compiler?: "compiling" | "success" | "error";
  private pending = false;
  private rendered = false;
  private held = false;
  private failed = false;

  get phase(): ActivityPhase {
    if (this.connection === "reconnecting") return "reconnecting";
    if (this.failed) return "render-error";
    if (this.connection !== "connected") return this.connection;
    if (this.compiler === "error") return "compile-error";
    if (this.pending) return this.held ? "selection-held" : "rendering";
    if (this.compiler === "compiling") return "compiling";
    return this.rendered ? "ready" : "waiting";
  }

  connecting() {
    this.connection = "connecting";
  }

  opened() {
    this.connection = "connected";
    this.compiler = undefined;
    this.pending = this.rendered = this.held = this.failed = false;
  }

  disconnected() {
    this.connection = "reconnecting";
    this.pending = false;
  }

  compile(status: string) {
    if (status !== "compiling" && status !== "success" && status !== "error") return;
    this.compiler = status;
    if (status === "compiling") this.rendered = false;
    if (status === "error") this.pending = false;
  }

  frame() {
    this.pending = true;
    this.held = this.failed = false;
  }

  renderActivity(activity: "rendering" | "selection-held") {
    if (this.pending) this.held = activity === "selection-held";
  }

  didRender() {
    if (!this.pending || this.failed || this.connection !== "connected") return;
    this.pending = this.held = this.failed = false;
    this.rendered = true;
  }

  renderError() {
    this.failed = true;
  }
}

const labels: Record<ActivityPhase, [string, string]> = {
  preparing: ["미리보기 준비 중…", "Preparing preview…"],
  connecting: ["서버에 연결 중…", "Connecting to server…"],
  waiting: ["문서를 기다리는 중…", "Waiting for document…"],
  compiling: ["문서 컴파일 중…", "Compiling document…"],
  rendering: ["화면에 그리는 중…", "Rendering document…"],
  "selection-held": ["선택을 해제하면 갱신됩니다", "Release selection to update"],
  reconnecting: ["연결 끊김 · 다시 연결 중…", "Disconnected · reconnecting…"],
  "compile-error": ["컴파일 오류 · 소스를 확인해 주세요", "Compilation failed · check source"],
  "render-error": [
    "미리보기를 표시하지 못했습니다 · 새로고침해 주세요",
    "Preview failed · reload to retry",
  ],
  ready: ["", ""],
};

export function activityPlacement(
  viewport: Pick<VisualViewport, "offsetLeft" | "offsetTop" | "width" | "scale">,
  insets: { left: number; right: number; top: number },
) {
  const scale = viewport.scale || 1;
  const left = Math.max(12, insets.left) / scale;
  const right = Math.max(12, insets.right) / scale;
  return {
    left: `${viewport.offsetLeft + (viewport.width + left - right) / 2}px`,
    top: `${viewport.offsetTop + (Math.max(12, insets.top) + 40) / scale}px`,
    maxWidth: `${Math.max(1, (viewport.width - left - right) * scale)}px`,
    transform: `translateX(-50%) scale(${1 / scale})`,
    transformOrigin: "top center",
  };
}

/** One indicator belongs to one connection lifetime, including its retries. */
export class PreviewActivity {
  readonly state = new PreviewActivityState();
  private element = document.getElementById("typst-preview-status")!;
  private label = this.element.querySelector<HTMLElement>(".typst-preview-status-label")!;
  private hint = this.element.querySelector<HTMLElement>(".typst-preview-status-hint")!;
  private phase?: ActivityPhase;
  private delayed?: ReturnType<typeof setTimeout>;
  private disposed = false;
  private korean = navigator.language.startsWith("ko");
  private position = () => {
    const viewport = window.visualViewport;
    if (!viewport) return;
    const style = getComputedStyle(document.documentElement);
    const inset = (edge: string) =>
      Number.parseFloat(style.getPropertyValue(`--preview-safe-${edge}`)) || 0;
    Object.assign(
      this.element.style,
      activityPlacement(viewport, {
        left: inset("left"),
        right: inset("right"),
        top: inset("top"),
      }),
    );
  };

  constructor() {
    window.visualViewport?.addEventListener("resize", this.position);
    window.visualViewport?.addEventListener("scroll", this.position);
    this.refresh();
  }

  refresh() {
    if (this.disposed) return;
    const phase = this.state.phase;
    if (phase !== this.phase) {
      this.phase = phase;
      clearTimeout(this.delayed);
      this.hint.hidden = true;
      this.element.hidden = phase === "ready";
      this.element.dataset.phase = phase;
      const busy = !["ready", "selection-held", "compile-error", "render-error"].includes(phase);
      this.element.dataset.busy = String(busy);
      this.label.textContent = labels[phase][this.korean ? 0 : 1];
      if (busy) {
        this.delayed = setTimeout(() => {
          if (this.disposed) return;
          this.hint.textContent = this.korean
            ? "평소보다 오래 걸리고 있습니다 · 서버 상태를 확인해 주세요"
            : "Taking longer than usual · check the server";
          this.hint.hidden = false;
        }, 15000);
      }
    }
    this.position();
  }

  dispose() {
    this.disposed = true;
    clearTimeout(this.delayed);
    window.visualViewport?.removeEventListener("resize", this.position);
    window.visualViewport?.removeEventListener("scroll", this.position);
    this.element.hidden = true;
  }
}
