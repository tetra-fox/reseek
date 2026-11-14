import type { Schemas } from "#lib/api/client.js";
import type { RowNote } from "#lib/config/sections.js";

/**
 * What the Relay settings say on the row of the agent configured under `entry`: that a device
 * other than the one connected keeps trying to log in as it, and which device that is
 */
export const agentNote = (
  relay: Schemas["RelayState"] | undefined,
  entry: string
): RowNote | undefined => {
  const refused = relay?.refusedDevices.filter((d) => d.entry === entry) ?? [];
  if (!refused.length) return undefined;
  return {
    text: "Another device is connected as this agent",
    detail: refused.map((d) => `Device ${d.device} from ${d.ipAddress} refused`).join(", ")
  };
};
