import type { Schemas } from "#lib/api/client.js";
import { getFileName, plural } from "#lib/format.js";

// one entry per type the daemon raises; the record's type makes a missing or stale one an error
const TYPES: Record<Schemas["EventType"], true> = {
  DownloadFileComplete: true,
  DownloadFileFailed: true,
  DownloadDirectoryComplete: true,
  UploadFileComplete: true,
  PrivateMessageReceived: true,
  RoomMessageReceived: true,
  UserBlocked: true,
  UserUnblocked: true,
  BuddyPresenceChanged: true,
  TokenCreated: true,
  TokenRevoked: true,
  SessionCreated: true,
  SessionRevoked: true,
  LoginFailed: true,
  FileAnalysisComplete: true,
  ScriptRunCompleted: true,
  SoulseekClientConnected: true,
  SoulseekClientDisconnected: true,
  RelayAgentConnected: true,
  RelayAgentDisconnected: true,
  RelayAgentSharesUpdated: true,
  JobFinished: true,
  Noop: true
};

/** The event types the log can be filtered by */
export const EVENT_TYPES = Object.keys(TYPES) as Schemas["EventType"][];

/** What an event's data column says: the user it is about, shown as a name, and the rest in words */
export type EventSummary = { user: string | null; text: string };

const SESSION_ENDS: Record<Schemas["SessionRevokeReason"], string> = {
  logout: "logged out",
  admin: "logged out by an administrator",
  backchannel: "ended by the identity provider",
  proxy: "ended by the reverse proxy",
  expired: "expired",
  configuration: "ended, no role in the configuration"
};

const JOB_ENDINGS: Record<Schemas["JobEnding"], string> = {
  succeeded: "succeeded",
  finished_with_failures: "finished with failures",
  failed: "failed",
  canceled: "canceled"
};

/** An event's data in a few words, for its line in the event log */
export const eventSummary = (e: Schemas["Event"]): EventSummary => {
  const say = (text: string, user: string | null = null) => ({ user, text });
  switch (e.type) {
    case "DownloadFileComplete":
    case "UploadFileComplete":
      return say(getFileName(e.data.remoteFilename), e.data.transfer.username);
    case "DownloadFileFailed":
      return say(
        `${getFileName(e.data.remoteFilename)}: ${e.data.reason}`,
        e.data.transfer.username
      );
    case "DownloadDirectoryComplete":
      return say(getFileName(e.data.localDirectoryName), e.data.username);
    case "PrivateMessageReceived":
      return say(e.data.message.message, e.data.message.username);
    case "RoomMessageReceived":
      return say(
        `in ${e.data.message.roomName}: ${e.data.message.message}`,
        e.data.message.username
      );
    case "UserBlocked":
      return say(e.data.reason || e.data.scopes.join(", "), e.data.username);
    case "UserUnblocked":
      return say(e.data.reason === "expired" ? "block expired" : "unblocked", e.data.username);
    case "BuddyPresenceChanged":
      return say(`${e.data.previous} to ${e.data.presence}`, e.data.username);
    case "TokenCreated":
    case "TokenRevoked":
      return say(`${e.data.name}, ${e.data.role}`);
    case "SessionCreated":
      return say(`${e.data.name}, ${e.data.role}${e.data.ip ? `, from ${e.data.ip}` : ""}`);
    case "SessionRevoked":
      return say(SESSION_ENDS[e.data.reason]);
    case "LoginFailed":
      return say(`${e.data.provider} from ${e.data.ip}: ${e.data.reason}`);
    case "FileAnalysisComplete":
      return say(`${getFileName(e.data.path)}: ${e.data.summary}`);
    case "ScriptRunCompleted": {
      const d = e.data;
      const outcome = d.succeeded
        ? "succeeded"
        : d.timedOut
          ? "timed out"
          : d.exitCode != null
            ? `failed with exit code ${d.exitCode}`
            : `failed${d.error ? `: ${d.error}` : ""}`;
      return say(`${d.script} for ${d.eventType} ${outcome}`);
    }
    case "SoulseekClientDisconnected":
      return say(
        e.data.exception ? `${e.data.message}: ${e.data.exception.message}` : e.data.message
      );
    case "RelayAgentConnected":
      return say(`${e.data.agent} from ${e.data.ipAddress}`);
    case "RelayAgentDisconnected":
      return say(`${e.data.agent}: ${e.data.reason}`);
    case "RelayAgentSharesUpdated":
      return say(
        `${e.data.agent}: ${plural(e.data.files, "file", "files")} in ${plural(e.data.directories, "folder", "folders")}`
      );
    case "JobFinished":
      return say(`${e.data.name}: ${e.data.summary || JOB_ENDINGS[e.data.ending]}`);
    case "SoulseekClientConnected":
    case "Noop":
      return say("");
  }
};

/**
 * An event's data as indented JSON, except that a string of several lines, such as a script's
 * output, is written out on its lines rather than as one line of `\n`s
 */
export const payloadText = (value: unknown, pad = ""): string => {
  const inner = `${pad}  `;
  if (typeof value === "string") {
    const lines = value.split("\n").map((line) => JSON.stringify(line).slice(1, -1));
    return `"${lines.join(`\n${inner}`)}"`;
  }
  if (Array.isArray(value)) {
    if (!value.length) return "[]";
    return `[\n${value.map((v) => inner + payloadText(v, inner)).join(",\n")}\n${pad}]`;
  }
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value);
    if (!entries.length) return "{}";
    const lines = entries.map(([k, v]) => `${inner}${JSON.stringify(k)}: ${payloadText(v, inner)}`);
    return `{\n${lines.join(",\n")}\n${pad}}`;
  }
  return JSON.stringify(value);
};
