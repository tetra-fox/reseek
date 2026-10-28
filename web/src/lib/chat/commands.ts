/** Where a command is typed: a room's composer or a conversation's */
export type Place = "room" | "user";

/** A chat command as `/help` lists it */
export type CommandInfo = {
  name: string;
  /** what follows the name, as `/help` shows it */
  args: string;
  about: string;
  /** typed only in a room */
  roomOnly?: boolean;
};

/** Every command, in the order `/help` lists them */
export const COMMANDS: CommandInfo[] = [
  { name: "help", args: "", about: "List these commands" },
  { name: "me", args: "<action>", about: "Say what you are doing: /me waves" },
  { name: "msg", args: "<user> <text>", about: "Send a private message" },
  { name: "join", args: "<room>", about: "Join a room, creating it if it doesn't exist" },
  { name: "leave", args: "[room]", about: "Leave this room, or the one named" },
  { name: "browse", args: "<user>", about: "Browse a user's shares" },
  { name: "whois", args: "<user>", about: "Open a user's profile" },
  { name: "search", args: "<text>", about: "Search the network" },
  { name: "rsearch", args: "<text>", about: "Search this room's members", roomOnly: true },
  { name: "block", args: "<user>", about: "Block a user" },
  { name: "clear", args: "", about: "Clear this view; the history is kept" }
];

/** What the composer's text asks for */
export type Parsed =
  /** a message to send as it is */
  | { kind: "text"; text: string }
  /** a command with what it needs: `target` a user or room, `text` the rest */
  | { kind: "command"; name: string; target: string; text: string }
  /** a command that can't run, which is said and not sent */
  | { kind: "error"; message: string };

/**
 * The first word of `rest` and what follows it; a word in double quotes may hold spaces, as
 * a username can
 */
const firstWord = (rest: string): [string, string] => {
  const quoted = /^"([^"]+)"\s*(.*)$/s.exec(rest);
  if (quoted) return [quoted[1], quoted[2]];
  const cut = rest.search(/\s/);
  return cut === -1 ? [rest, ""] : [rest.slice(0, cut), rest.slice(cut).trimStart()];
};

const usage = (c: CommandInfo) => `/${c.name}${c.args ? ` ${c.args}` : ""}`;

/**
 * Reads what was typed. Text that doesn't start with `/` is a message, and `//` sends a message
 * starting with one `/`. `/me` is a message too, sent as typed, which other clients show as an
 * action. A command missing what it needs, typed where it doesn't apply or not known is an
 * error, so it is never sent by mistake
 */
export const parseCommand = (input: string, place: Place): Parsed => {
  const text = input.trim();
  if (!text.startsWith("/")) return { kind: "text", text };
  if (text.startsWith("//")) return { kind: "text", text: text.slice(1) };
  const match = /^\/(\S+)\s*(.*)$/s.exec(text);
  const name = match?.[1].toLowerCase() ?? "";
  const rest = match?.[2].trim() ?? "";
  const command = COMMANDS.find((c) => c.name === name);
  if (!command)
    return { kind: "error", message: `/${name} isn't a command. Type /help to see the list.` };
  if (command.roomOnly && place !== "room") {
    return { kind: "error", message: `/${name} only works in a room` };
  }
  const needs = command.args.startsWith("<");
  if (needs && !rest) return { kind: "error", message: `Usage: ${usage(command)}` };
  if (name === "me") return { kind: "text", text: `/me ${rest}` };
  if (name === "msg") {
    const [target, message] = firstWord(rest);
    if (!message) return { kind: "error", message: `Usage: ${usage(command)}` };
    return { kind: "command", name, target, text: message };
  }
  if (name === "leave" && !rest && place !== "room") {
    return { kind: "error", message: "Name the room to leave: /leave <room>" };
  }
  const takesTarget = ["join", "leave", "browse", "whois", "block"].includes(name);
  return takesTarget
    ? { kind: "command", name, target: rest.replace(/^"(.*)"$/, "$1"), text: "" }
    : { kind: "command", name, target: "", text: rest };
};

/** The commands whose names start with what was typed after a `/`, for completion */
export const commandsFor = (typed: string, place: Place): CommandInfo[] =>
  COMMANDS.filter(
    (c) => c.name.startsWith(typed.toLowerCase()) && (!c.roomOnly || place === "room")
  );
