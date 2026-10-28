import type { Schemas } from "#lib/api/client.js";
import { mappingText, reachableText } from "#lib/reachability.js";

/**
 * What a bug report needs about this reseek, as plain text to paste: versions, the connection and
 * how peers reach us, shares, the distributed network, what waits on a restart, and the latest
 * warnings and errors. The account name, addresses and paths are left out
 */
export const diagnostics = (
  s: Schemas["State"],
  browser: string,
  problems: Schemas["LogRecord"][] = []
): string => {
  const d = s.distributedNetwork;
  const lines = [
    `reseek ${s.version.full}${s.version.isUpdateAvailable && s.version.latest ? ` (latest ${s.version.latest})` : ""}`,
    `browser: ${browser}`,
    `server: ${s.server.state}${s.connectionWatchdog.isEnabled ? ", reconnects on" : ""}`,
    `listen: port ${s.server.listen.port}; ${mappingText(s.server.listen)} ${reachableText(s.server.listen)}`,
    `shares: ${s.shares.files} files in ${s.shares.directories} folders${s.shares.scanning ? ", scanning" : ""}${s.shares.scanPending ? ", rescan pending" : ""}${s.shares.faulted ? ", last scan failed" : ""}`,
    `distributed: ${d.hasParent ? "parent" : "no parent"}, level ${d.branchLevel}, ${d.children.length}/${d.childLimit} children`,
    `searches from others: ${s.health.search.incoming.queueDepth} waiting, ${Math.round(s.health.search.incoming.latency)} ms`,
    `relay: ${s.relay.mode}`,
    `pending: ${[s.pendingRestart && "restart", s.pendingReconnect && "reconnect"].filter(Boolean).join(", ") || "nothing"}`
  ];
  if (s.configuration.lastReloadError) {
    lines.push(`last options reload: ${s.configuration.lastReloadError}`);
  }
  if (problems.length) {
    lines.push("", "recent warnings and errors:");
    for (const r of problems) {
      lines.push(`${r.timestamp} ${r.level} ${r.context}: ${r.message.split("\n")[0]}`);
    }
  }
  return lines.join("\n");
};

/** How many of the latest warnings and errors diagnostics carry */
export const PROBLEMS = 20;

/** The latest warnings and errors of a log, oldest first */
export const latestProblems = (records: Schemas["LogRecord"][]) =>
  records.filter((r) => ["Warning", "Error", "Fatal"].includes(r.level)).slice(-PROBLEMS);
