import type { Schemas } from "#lib/api/client.js";

export type Listen = Schemas["ListenState"];

/** Where the port and its mapping are set */
export const SOULSEEK_SETTINGS = "/system/settings?section=soulseek";

/** What the search screen and the connection button say when other users can't reach us */
export const unreachable = (port: number) =>
  `Other users can't connect to you, so you'll see fewer search results and some downloads won't start. Forward TCP port ${port} to this computer, or turn on port mapping.`;

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
      return "Port mapping is off: reseek doesn't ask the router to forward the port.";
    case "mapping":
      return `Asking the router to forward port ${listen.port}...`;
    case "mapped": {
      const how = m.method ? ` with ${METHODS[m.method]}` : "";
      const where = m.externalAddress
        ? ` as ${m.externalAddress}:${m.externalPort ?? listen.port}`
        : "";
      return `The router forwards port ${listen.port} to reseek${how}${where}.`;
    }
    case "failed":
      return `The router didn't forward port ${listen.port}: ${m.error ?? "no reason given"}`;
  }
};

/** Whether other users can connect to us, in a sentence */
export const reachableText = (listen: Listen): string =>
  listen.reachable === "yes"
    ? "Other users connect to you."
    : listen.reachable === "no"
      ? "Other users haven't been able to connect to you."
      : "Not known yet whether other users can connect to you.";
