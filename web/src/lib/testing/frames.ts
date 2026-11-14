import type { Schemas } from "#lib/api/client.js";

/** The run of the daemon the fake frames come from, 16 hex digits as the daemon's are */
export const EPOCH: Schemas["Hello"]["epoch"] = "00000000000000e2";

/**
 * One event stream frame as the daemon writes it, the `seq`th of the run `epoch`, whose id is the
 * run and the number. A stream's hello and snapshots take the number of the last frame published
 * before it opened
 */
export const frameText = (
  seq: number,
  event: string,
  data: unknown,
  epoch: Schemas["Hello"]["epoch"] = EPOCH
) => `id: ${epoch}-${seq}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
