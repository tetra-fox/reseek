import {
  isActive,
  isMoving,
  isRetryable,
  isSucceeded,
  type Transfer
} from "#lib/transfers/states.js";

/** How far a group of transfers has come, counting those still wanted: not failed or canceled */
export type Progress = {
  /** bytes moved, a finished file counting whole */
  moved: number;
  /** bytes wanted in all */
  size: number;
  /** 0 to 100 */
  percent: number;
  /**
   * seconds until the rest is moved: the time left of its slowest file, while every file not yet
   * done is moving. Null while one waits or has no time left to tell, as nothing says when it ends
   */
  eta: number | null;
  /** some of it is running */
  running: boolean;
};

export const groupProgress = (transfers: Transfer[]): Progress => {
  let moved = 0;
  let size = 0;
  let running = false;
  let left = 0;
  let known = true;
  for (const t of transfers) {
    if (isRetryable(t.state)) continue;
    size += t.size;
    if (isSucceeded(t.state)) {
      moved += t.size;
      continue;
    }
    moved += Math.min(t.bytesTransferred, t.size);
    if (isActive(t.state)) running = true;
    if (isMoving(t.state) && t.remainingTime !== null && t.remainingTime !== undefined)
      left = Math.max(left, t.remainingTime);
    else known = false;
  }
  return {
    moved,
    size,
    percent: size ? (moved / size) * 100 : 0,
    eta: known && left > 0 ? Math.ceil(left / 1000) : null,
    running
  };
};
