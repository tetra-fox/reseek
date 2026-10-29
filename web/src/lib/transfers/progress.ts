import { isActive, isRetryable, isSucceeded, type Transfer } from "./states";

/** How far a group of transfers has come, counting those still wanted: not failed or canceled */
export type Progress = {
  /** bytes moved, a finished file counting whole */
  moved: number;
  /** bytes wanted in all */
  size: number;
  /** 0 to 100 */
  percent: number;
  /** what the running transfers move together, bytes a second */
  speed: number;
  /** seconds until the rest is moved at that speed, null when nothing moves */
  eta: number | null;
  /** some of it is running */
  running: boolean;
};

export const groupProgress = (transfers: Transfer[]): Progress => {
  let moved = 0;
  let size = 0;
  let speed = 0;
  let running = false;
  for (const t of transfers) {
    if (isRetryable(t.state)) continue;
    size += t.size;
    moved += isSucceeded(t.state) ? t.size : Math.min(t.bytesTransferred, t.size);
    if (isActive(t.state)) {
      running = true;
      speed += t.averageSpeed;
    }
  }
  return {
    moved,
    size,
    percent: size ? (moved / size) * 100 : 0,
    speed,
    eta: speed > 0 ? Math.ceil((size - moved) / speed) : null,
    running
  };
};
