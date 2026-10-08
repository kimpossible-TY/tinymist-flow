/** Classifies native touch gestures without canceling browser selection. */
export class TouchTap {
  private start?: { x: number; y: number; at: number };
  private blocked = true;

  begin(x: number, y: number, at: number, count: number, selected: boolean) {
    this.start = { x, y, at };
    this.blocked = count !== 1 || selected;
  }

  move(x: number, y: number, count: number) {
    if (!this.start || count !== 1 || Math.hypot(x - this.start.x, y - this.start.y) > 8)
      this.blocked = true;
  }

  cancel() {
    this.blocked = true;
  }

  consume(at: number, selected: boolean) {
    const accepted = !!this.start && !this.blocked && !selected && at - this.start.at <= 500;
    this.blocked = true;
    return accepted;
  }
}
