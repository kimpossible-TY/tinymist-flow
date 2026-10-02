export type ChangeLocation = [number, number, number];

export function parseChangeLocation(payload: string): ChangeLocation | undefined {
  const location = payload.trim().split(/\s+/).map(Number);
  if (
    location.length !== 3 ||
    !location.every(Number.isFinite) ||
    !Number.isSafeInteger(location[0]) ||
    location[0] < 1 ||
    location[1] < 0 ||
    location[2] < 0
  )
    return undefined;
  return location as ChangeLocation;
}

/** Ordered hints wait for rendering; a palette transition retains its reading state. */
export class ChangeLocationQueue {
  private pending?: ChangeLocation;
  lastLocation?: ChangeLocation;

  constructor(private readonly preserveReadingState: boolean) {}

  queue(location: ChangeLocation, resume = false): void {
    if (resume && this.preserveReadingState) return;
    this.pending = location;
  }

  clear(): void {
    this.pending = undefined;
    this.lastLocation = undefined;
  }

  afterRender(
    now: number,
    lastGesture: number,
  ): { location: ChangeLocation; deferred: boolean } | undefined {
    if (!this.pending) return undefined;
    this.lastLocation = this.pending;
    this.pending = undefined;
    return { location: this.lastLocation, deferred: now - lastGesture < 2500 };
  }
}
