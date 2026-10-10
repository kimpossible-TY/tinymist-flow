import { afterEach, describe, expect, it, vi } from "vitest";
import {
  captureReferenceOrigin,
  ReferenceNavigation,
  restoreReferenceOrigin,
} from "./reference-navigation";

function fixture() {
  let scale = 1;
  let count = 4;
  const viewport = { top: 30, left: 20 };
  const scroller = {
    style: {
      value: "",
      getPropertyValue: () => scroller.style.value,
      getPropertyPriority: () => "",
      setProperty: (_key: string, value: string) => {
        scroller.style.value = value;
      },
      removeProperty: () => {
        scroller.style.value = "";
      },
    },
    scrollTop: 840,
    scrollLeft: 80,
    getBoundingClientRect: () => viewport,
    scrollTo: vi.fn((position: ScrollToOptions) => {
      scroller.scrollTop = position.top ?? scroller.scrollTop;
      scroller.scrollLeft = position.left ?? scroller.scrollLeft;
    }),
  };
  const pages = Array.from({ length: 4 }, (_, page) => ({
    getAttribute: () => String(page),
    getBoundingClientRect: () => ({
      top: viewport.top + page * 1000 * scale - scroller.scrollTop,
      bottom: viewport.top + (page + 1) * 1000 * scale - scroller.scrollTop,
      left: viewport.left - scroller.scrollLeft,
      width: 600 * scale,
      height: 1000 * scale,
    }),
  }));
  const link = {} as Element;
  const root = {
    contains: (element: Element) => element === link,
    querySelectorAll: () => pages.slice(0, count),
    querySelector: (selector: string) => {
      const page = Number(/data-page-number="(\d+)"/.exec(selector)?.[1]);
      return page < count ? pages[page] : null;
    },
  };
  const button = Object.assign(new EventTarget(), { hidden: false, remove: vi.fn() });
  vi.stubGlobal("document", { createElement: () => button });
  vi.stubGlobal("navigator", { language: "ko-KR" });
  const previous = vi.fn((_element: Element, page: number) => {
    if (page >= 1 && page <= count) scroller.scrollTop = (page - 1) * 1000 * scale;
  });
  vi.stubGlobal("window", { handleTypstLocation: previous });
  let frame: FrameRequestCallback | undefined;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frame = callback;
    return 1;
  });
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  const onNavigate = vi.fn();
  const onRestore = vi.fn();
  const navigation = new ReferenceNavigation(
    scroller as unknown as HTMLElement,
    root as unknown as HTMLElement,
    { appendChild: vi.fn() } as unknown as HTMLElement,
    onNavigate,
    onRestore,
  );
  return {
    scroller,
    root: root as unknown as HTMLElement,
    button,
    previous,
    navigation,
    onNavigate,
    onRestore,
    follow: (page: number) => window.handleTypstLocation(link, page, 10, 20),
    return: () => button.dispatchEvent(new Event("click")),
    frame: () => frame?.(0),
    resize: (nextScale: number) => {
      scale = nextScale;
    },
    removePages: () => {
      count = 0;
    },
  };
}

afterEach(() => vi.unstubAllGlobals());

describe("return from an internal reference", () => {
  it("restores both scroll offsets once and hides the button", () => {
    const view = fixture();
    expect(view.button.hidden).toBe(true);
    view.follow(4);
    expect(view.scroller.scrollTop).toBe(3000);
    expect(view.button.hidden).toBe(false);
    expect(view.onNavigate).toHaveBeenCalledOnce();
    view.return();
    expect(view.scroller.scrollTo).toHaveBeenLastCalledWith({
      top: 840,
      left: 80,
      behavior: "instant",
    });
    expect(view.onRestore).toHaveBeenCalledOnce();
    expect(view.button.hidden).toBe(true);
    view.return();
    expect(view.scroller.scrollTo).toHaveBeenCalledOnce();
  });

  it("replaces the origin with the most recent reference without a back stack", () => {
    const view = fixture();
    view.follow(3);
    view.scroller.scrollTop = 2345;
    view.follow(4);
    view.return();
    expect(view.scroller.scrollTop).toBeCloseTo(2345);
    expect(view.button.hidden).toBe(true);
    view.return();
    expect(view.scroller.scrollTo).toHaveBeenCalledOnce();
  });

  it("retains the page-relative passage after zoom or viewport resizing", () => {
    const view = fixture();
    view.follow(4);
    view.resize(1.5);
    view.return();
    expect(view.scroller.scrollTop).toBeCloseTo(1260);
    expect(view.scroller.scrollLeft).toBeCloseTo(120);
  });

  it("does not create a return for editor navigation or invalid internal targets", () => {
    const view = fixture();
    view.previous({} as Element, 3);
    expect(view.button.hidden).toBe(true);
    view.follow(999);
    window.handleTypstLocation({} as Element, 2, 0, 0);
    expect(view.button.hidden).toBe(true);
    expect(view.onNavigate).not.toHaveBeenCalled();
  });

  it("keeps the reference origin when another navigation changes the viewport", () => {
    const view = fixture();
    view.follow(3);
    view.previous({} as Element, 4);
    view.return();
    expect(view.scroller.scrollTop).toBeCloseTo(840);
  });

  it("dismisses a return when its page no longer exists", () => {
    const view = fixture();
    view.follow(4);
    view.removePages();
    view.return();
    expect(view.button.hidden).toBe(true);
    expect(view.scroller.scrollTo).not.toHaveBeenCalled();
    expect(view.onRestore).not.toHaveBeenCalled();
  });

  it("removes the button and restores the global handler on disposal", () => {
    const view = fixture();
    view.follow(4);
    view.navigation.dispose();
    expect(view.button.remove).toHaveBeenCalledOnce();
    expect(window.handleTypstLocation).toBe(view.previous);
    view.return();
    expect(view.scroller.scrollTo).not.toHaveBeenCalled();
  });

  it("cannot capture or restore an unrendered document", () => {
    const view = fixture();
    view.removePages();
    const scroller = view.scroller as unknown as HTMLElement;
    expect(captureReferenceOrigin(scroller, view.root)).toBeUndefined();
    expect(restoreReferenceOrigin({ page: 0, x: 0, y: 0 }, scroller, view.root)).toBe(false);
  });

  it("holds browser scroll anchoring until the returned page has rendered", () => {
    const view = fixture();
    view.follow(4);
    view.return();
    expect(view.scroller.style.value).toBe("none");
    view.navigation.afterRender();
    expect(view.scroller.style.value).toBe("none");
    view.frame();
    expect(view.scroller.style.value).toBe("");
    view.scroller.style.value = "auto";
    view.follow(4);
    view.return();
    view.navigation.dispose();
    expect(view.scroller.style.value).toBe("auto");
  });
});
