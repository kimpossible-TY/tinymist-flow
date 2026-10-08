import type { SelectedText, TextFragment } from "./text-selection";

export interface ReviewMark {
  fragments: TextFragment[];
  fingerprints: Record<number, string>;
}

const MAX_MARKS = 100;
const MAX_BYTES = 1024 * 1024;

/** Browser-local, bounded storage; marks never enter the Typst compiler. */
export class ReviewMarks {
  marks: ReviewMark[] = [];
  private key?: string;
  constructor(private storage?: Pick<Storage, "getItem" | "setItem">) {}

  setDocument(id: string) {
    const key = `tinymist-review-v1:${id}`;
    if (this.key === key) return;
    this.key = key;
    this.marks = [];
    try {
      const raw = this.storage?.getItem(key);
      if (!raw || raw.length > MAX_BYTES) return;
      const saved: unknown = JSON.parse(raw);
      if (Array.isArray(saved) && saved.length <= MAX_MARKS && saved.every(validMark))
        this.marks = saved;
    } catch {
      /* Private browsing, corrupt storage and quota failures retain memory use. */
    }
  }

  add(selection: SelectedText) {
    this.marks.push({ fragments: selection.fragments, fingerprints: selection.fingerprints });
    this.marks = this.marks.slice(-MAX_MARKS);
    this.save();
  }

  undo() {
    this.marks.pop();
    this.save();
  }
  clear() {
    this.marks = [];
    this.save();
  }

  /** Virtualized pages are unknown, not changed. */
  reconcile(pages: Map<number, { fingerprint: string }>) {
    const before = this.marks.length;
    this.marks = this.marks.filter((mark) =>
      Object.entries(mark.fingerprints).every(([page, fingerprint]) => {
        const current = pages.get(Number(page))?.fingerprint;
        return !current || current === fingerprint;
      }),
    );
    if (before !== this.marks.length) this.save();
  }

  private save() {
    if (!this.key) return;
    let serialized = JSON.stringify(this.marks);
    while (serialized.length > MAX_BYTES && this.marks.length) {
      this.marks.shift();
      serialized = JSON.stringify(this.marks);
    }
    try {
      this.storage?.setItem(this.key, serialized);
    } catch {
      /* Memory still works. */
    }
  }
}

function validMark(mark: unknown): mark is ReviewMark {
  if (!mark || typeof mark !== "object") return false;
  const value = mark as ReviewMark;
  return (
    Array.isArray(value.fragments) &&
    value.fragments.length > 0 &&
    value.fragments.length <= 256 &&
    !!value.fingerprints &&
    typeof value.fingerprints === "object" &&
    value.fragments.every(
      (fragment) =>
        Number.isInteger(fragment.page) &&
        fragment.page > 0 &&
        [fragment.x, fragment.y, fragment.width, fragment.height].every(Number.isFinite) &&
        fragment.x >= 0 &&
        fragment.y >= 0 &&
        fragment.width > 0 &&
        fragment.height > 0 &&
        typeof value.fingerprints[fragment.page] === "string",
    )
  );
}
