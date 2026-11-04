import { describe, expect, it } from "vitest";

import type { Schemas } from "#lib/api/client.js";

import { diagnostics, latestProblems, PROBLEMS } from "./diagnostics";

const state = {
  version: { full: "0.1.0+abc", latest: "0.2.0", isUpdateAvailable: true },
  server: {
    state: "Connected, LoggedIn",
    address: "server.slsknet.org",
    ipEndPoint: "203.0.113.9:2242",
    listen: {
      port: 2234,
      portMapping: {
        state: "mapped",
        method: "upnp",
        externalAddress: "198.51.100.7",
        externalPort: 2234,
        error: "the router at 192.168.20.1 took a while"
      },
      reachable: "yes"
    }
  },
  connectionWatchdog: { isEnabled: true },
  user: { username: "secret-account" },
  shares: { files: 1200, directories: 80, scanning: false, scanPending: true, faulted: false },
  distributedNetwork: { hasParent: true, branchLevel: 2, children: ["x"], childLimit: 10 },
  health: { search: { incoming: { queueDepth: 3, latency: 12.4 } } },
  relay: { enabled: false, agents: [] },
  pendingRestart: true,
  pendingReconnect: false,
  configuration: { lastReloadError: null }
} as unknown as Schemas["State"];

const record = (n: number, level: Schemas["LogLevel"]): Schemas["LogRecord"] => ({
  timestamp: `2026-10-27T18:00:${String(n % 60).padStart(2, "0")}Z`,
  context: "reseek::network",
  subContext: null,
  level,
  message: `line ${n}\nwith a stack`
});

describe("the diagnostics to paste in a bug report", () => {
  it("say how this reseek stands, leaving out the account and addresses", () => {
    const text = diagnostics(state, "Chrome 140", [record(1, "Warning")]);
    expect(text).toContain("reseek 0.1.0+abc (latest 0.2.0)");
    expect(text).toContain("server: Connected, LoggedIn, reconnects on");
    expect(text).toContain("shares: 1200 files in 80 folders, rescan pending");
    expect(text).toContain("pending: restart");
    expect(text).toContain("listen: port 2234, port mapping mapped (upnp), reachable yes");
    expect(text).toContain("2026-10-27T18:00:01Z Warning reseek::network: line 1");
    expect(text).not.toContain("with a stack");
    for (const secret of [
      "secret-account",
      "203.0.113.9",
      "server.slsknet.org",
      "198.51.100.7",
      "192.168.20.1"
    ]) {
      expect(text).not.toContain(secret);
    }
  });

  it("carry the latest warnings and errors only", () => {
    const records = Array.from({ length: 50 }, (_, n) =>
      record(n, n % 2 ? "Information" : n % 4 ? "Warning" : "Error")
    );
    const picked = latestProblems(records);
    expect(picked).toHaveLength(PROBLEMS);
    expect(picked.every((r) => r.level !== "Information")).toBe(true);
    expect(picked.at(-1)).toBe(records[48]);
  });
});
