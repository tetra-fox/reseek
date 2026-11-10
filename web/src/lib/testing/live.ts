import type { Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";

import { fakeNetwork } from "./network";

/**
 * A fake network, as `fakeNetwork` makes it, with the event stream open and greeted as the daemon
 * greets every stream, for a client signed in with `role`; `greet` sends that hello again, as a
 * stream opened anew without the frames missed, from the run `epoch` of the daemon, the same
 * run unless given. Undone by `live.stop()` and `vi.unstubAllGlobals()`
 */
export const connected = (
  answer: Parameters<typeof fakeNetwork>[0],
  role: Schemas["Role"] = "ReadOnly"
) => {
  const net = fakeNetwork(answer);
  const greet = (epoch = "first") =>
    net.frame("hello", {
      principal: { name: "me", role },
      resumed: false,
      epoch,
      serverTime: "2026-10-26T10:00:00Z",
      topics: [],
      version: { current: "0.1.0", full: "0.1.0", isCanary: false, isDevelopment: true }
    });
  live.start();
  greet();
  return { ...net, greet };
};
