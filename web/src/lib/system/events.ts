import type { Schemas } from "#lib/api/client.js";

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
  Noop: true
};

/** The event types the log can be filtered by */
export const EVENT_TYPES = Object.keys(TYPES) as Schemas["EventType"][];
