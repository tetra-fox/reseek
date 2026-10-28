import { describe, expect, it } from "vitest";

import { commandsFor, parseCommand } from "./commands";

describe("chat commands", () => {
  it("leave messages alone, and send // as one /", () => {
    expect(parseCommand("  hello  ", "room")).toEqual({ kind: "text", text: "hello" });
    expect(parseCommand("//join is a command", "room")).toEqual({
      kind: "text",
      text: "/join is a command"
    });
  });

  it("send /me as typed, for other clients to show as an action", () => {
    expect(parseCommand("/me waves", "user")).toEqual({ kind: "text", text: "/me waves" });
    expect(parseCommand("/me", "user")).toEqual({ kind: "error", message: "Usage: /me <action>" });
  });

  it("read each command's user, room or text, names with spaces included", () => {
    expect(parseCommand("/join The Lobby", "user")).toEqual({
      kind: "command",
      name: "join",
      target: "The Lobby",
      text: ""
    });
    expect(parseCommand("/msg alice see you", "room")).toEqual({
      kind: "command",
      name: "msg",
      target: "alice",
      text: "see you"
    });
    expect(parseCommand('/msg "dj shadow" hi there', "room")).toMatchObject({
      target: "dj shadow",
      text: "hi there"
    });
    expect(parseCommand("/whois dj shadow", "room")).toMatchObject({ target: "dj shadow" });
    expect(parseCommand("/BROWSE bob", "room")).toMatchObject({ name: "browse", target: "bob" });
    expect(parseCommand("/search boards of canada", "user")).toEqual({
      kind: "command",
      name: "search",
      target: "",
      text: "boards of canada"
    });
    expect(parseCommand("/leave", "room")).toMatchObject({ name: "leave", target: "" });
    expect(parseCommand("/clear", "user")).toMatchObject({ kind: "command", name: "clear" });
  });

  it("say what is wrong rather than send a command", () => {
    expect(parseCommand("/dance", "room")).toEqual({
      kind: "error",
      message: "/dance is not a command; /help lists them"
    });
    expect(parseCommand("/msg alice", "room")).toEqual({
      kind: "error",
      message: "Usage: /msg <user> <text>"
    });
    expect(parseCommand("/join", "room")).toEqual({
      kind: "error",
      message: "Usage: /join <room>"
    });
    expect(parseCommand("/rsearch x", "user")).toEqual({
      kind: "error",
      message: "/rsearch works in a room"
    });
    expect(parseCommand("/leave", "user").kind).toBe("error");
  });

  it("complete names, a room's own commands only in a room", () => {
    expect(commandsFor("s", "room").map((c) => c.name)).toEqual(["search"]);
    expect(commandsFor("r", "room").map((c) => c.name)).toEqual(["rsearch"]);
    expect(commandsFor("r", "user")).toEqual([]);
    expect(commandsFor("", "user").map((c) => c.name)).toContain("help");
  });
});
