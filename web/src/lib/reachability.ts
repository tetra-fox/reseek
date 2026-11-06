import type { Schemas } from "#lib/api/client.js";

export type Listen = Schemas["ListenState"];

/** Where the connection to the Soulseek server stands, as a word or two */
export const serverStateText = (s: Schemas["ServerState"]): string =>
  s.isLoggedIn
    ? "Logged in"
    : s.isLoggingIn
      ? "Logging in"
      : s.isConnected
        ? "Connected"
        : s.isConnecting
          ? "Connecting"
          : "Disconnected";

/** Where the port and its mapping are set */
export const SOULSEEK_SETTINGS = "/system/settings?section=soulseek";

/** What the search screen says when other users can't reach us */
export const unreachable = (port: number) =>
  `Other users can't connect to you, so searches return fewer results and some downloads won't start. Forward TCP port ${port} to this computer or turn on port mapping.`;

/** What the connection button's tooltip says when other users can't reach us */
export const UNREACHABLE_HINT = "Other users cannot connect to you. Check port forwarding.";

const METHODS: Record<Schemas["PortMappingMethod"], string> = {
  upnp: "UPnP",
  natpmp: "NAT-PMP",
  pcp: "PCP"
};

/** Where asking the router to forward our port stands, in a sentence */
export const mappingText = (listen: Listen): string => {
  const m = listen.portMapping;
  switch (m.state) {
    case "off":
      return "Port mapping is off";
    case "mapping":
      return `Mapping port ${listen.port} on the router...`;
    case "mapped": {
      const how = m.method ? ` with ${METHODS[m.method]}` : "";
      const where = m.externalAddress
        ? ` as ${m.externalAddress}:${m.externalPort ?? listen.port}`
        : "";
      return `Port ${listen.port} is forwarded to reseek${how}${where}`;
    }
    case "failed":
      return `The router did not forward port ${listen.port}: ${m.error ?? "no reason given"}`;
  }
};

/** Whether other users can connect to us, in a sentence */
export const reachableText = (listen: Listen): string =>
  listen.reachable === "yes"
    ? "Other users can connect to you"
    : listen.reachable === "no"
      ? "Other users have not been able to connect to you"
      : "It is not known yet whether other users can connect to you";
