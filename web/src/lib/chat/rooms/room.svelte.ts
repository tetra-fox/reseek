import { api, byRoom, type Schemas } from "#lib/api/client.js";
import { Keyed } from "#lib/lists/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call, type Outcome } from "#lib/request.js";

import { addChange, type Burst, type PresenceKind } from "./presence";

export type RoomUser = Schemas["RoomUser"];

/**
 * One member's ticker on the room's wall, with when it was last seen to change and whether it is
 * hidden along with the member's room messages
 */
export type WallLine = { username: string; ticker: string; hidden: boolean; at: number };

/** How many folded lines of presence changes a room keeps, the oldest going first */
const KEPT = 500;

let seq = 0;

/**
 * Who is in a joined room and who runs it: read once, then kept by the room's frames, a member
 * joining or leaving patching the list in place. What happens to the members while the pane is
 * open (joins, leaves, going away and coming back, changes to a private room's members and
 * operators) is kept for the room's activity pane, folded as it arrives; it is not stored, so a
 * reload starts without it
 */
export class RoomMembers {
  readonly name: string;
  /** who is in it now, by username */
  readonly users = new Keyed<RoomUser>((u) => u.username);
  /** the room as last read or pushed: owner, operators, tickers */
  room = $state.raw<Schemas["Room"] | null>(null);
  /** why the last read failed, empty when it didn't or a push brought the whole room since */
  error = $state("");
  /** what happened to the members since the pane opened, folded into bursts, oldest first */
  bursts = $state<Burst[]>([]);
  /** the members' tickers, the latest change first */
  wall = $state.raw<WallLine[]>([]);
  /** counts the pushes of the whole room, each newer than any read already on its way */
  #pushes = 0;

  constructor(name: string) {
    this.name = name;
  }

  /** Follows the room's frames; call from a component's setup */
  follow() {
    $effect(() =>
      live.on("rooms.update", (room) => {
        if (room.name !== this.name) return;
        this.#pushes += 1;
        this.error = "";
        this.#take(room, true);
      })
    );
    $effect(() =>
      live.on("rooms.user_joined", ({ roomName, user }) => {
        if (roomName !== this.name) return;
        // one marked gone while the list is held is back, and joined again all the same
        const known = this.users.get(user.username) && !this.users.removed.has(user.username);
        this.users.upsert(user);
        if (!known) this.#note("joined", user.username);
      })
    );
    $effect(() =>
      live.on("rooms.user_left", ({ roomName, username }) => {
        if (roomName !== this.name) return;
        this.users.remove(username);
        this.#note("left", username);
      })
    );
    $effect(() =>
      live.on("rooms.user_status", ({ roomName, username, status }) => {
        if (roomName !== this.name) return;
        const user = this.users.get(username);
        if (!user || user.status === status) return;
        const was = user.status;
        this.users.assign(username, { status });
        if (status === "Away" && was === "Online") this.#note("away", username);
        else if (status === "Online" && was === "Away") this.#note("returned", username);
      })
    );
  }

  /** Reads the room and its users */
  load = async () => {
    const turn = this.users.reading();
    const pushes = this.#pushes;
    const outcome = await call(() =>
      api.GET("/api/v0/rooms/joined/{roomName}", { params: byRoom(this.name) })
    );
    // a later read, or a push of the whole room, is newer than this answer
    if (this.#pushes !== pushes || !this.users.latest(turn)) return void this.users.abandon(turn);
    if (!outcome.ok) {
      this.users.abandon(turn);
      this.error = outcome.error;
      return;
    }
    this.error = "";
    this.#take(outcome.data, false, turn);
  };

  /** The owner and operators of a private room */
  get operators(): Set<string> {
    const r = this.room;
    return new Set([...(r?.operators ?? []), ...(r?.owner ? [r.owner] : [])]);
  }

  /** Sets our ticker in the room; an empty text takes it down */
  setTicker = (text: string): Promise<Outcome<unknown>> =>
    call(() =>
      api.POST("/api/v0/rooms/joined/{roomName}/ticker", {
        params: byRoom(this.name),
        body: text
      })
    );

  /** Adds a member to the private room; for its owner and operators */
  addMember = (username: string) =>
    call(() =>
      api.POST("/api/v0/rooms/joined/{roomName}/members", {
        params: byRoom(this.name),
        body: username
      })
    );

  /** Takes a member out of the private room; owners may take out operators too */
  removeMember = (username: string) =>
    call(() =>
      api.DELETE("/api/v0/rooms/joined/{roomName}/members/{username}", {
        params: {
          path: { roomName: this.name, username },
          query: { roomName: this.name, username }
        }
      })
    );

  /** Makes a member an operator; for the owner */
  addOperator = (username: string) =>
    call(() =>
      api.POST("/api/v0/rooms/joined/{roomName}/operators", {
        params: byRoom(this.name),
        body: username
      })
    );

  /** Takes an operator back to member; for the owner */
  removeOperator = (username: string) =>
    call(() =>
      api.DELETE("/api/v0/rooms/joined/{roomName}/operators/{username}", {
        params: {
          path: { roomName: this.name, username },
          query: { roomName: this.name, username }
        }
      })
    );

  /** Gives up our membership of the private room for good */
  dropMembership = () =>
    call(() =>
      api.DELETE("/api/v0/rooms/private/{roomName}/membership", { params: byRoom(this.name) })
    );

  /** Gives up our ownership of the private room, which turns it public, and leaves it */
  dropOwnership = () =>
    call(() =>
      api.DELETE("/api/v0/rooms/private/{roomName}/ownership", { params: byRoom(this.name) })
    );

  /** What `us` may do in the room: own it, operate it, or neither */
  roleOf(us: string): "owner" | "operator" | null {
    const r = this.room;
    if (!r?.isPrivate || !us) return null;
    if (r.owner === us) return "owner";
    return r.operators?.includes(us) ? "operator" : null;
  }

  /**
   * Takes the room as read or pushed. A pushed one also says what changed in a private room's
   * members and operators; a read one only replaces what is shown, and is the answer of the
   * reload with `turn`
   */
  #take(room: Schemas["Room"], pushed: boolean, turn?: number) {
    const before = this.room;
    if (pushed && before?.isPrivate && room.isPrivate) {
      this.#diff(before.members ?? [], room.members ?? [], "added", "removed");
      this.#diff(before.operators ?? [], room.operators ?? [], "operator", "notOperator");
    }
    this.room = room;
    this.users.reconcile(room.users, turn);
    this.#wall(room.tickers);
  }

  #diff(before: string[], after: string[], added: PresenceKind, removed: PresenceKind) {
    const was = new Set(before);
    const now = new Set(after);
    for (const name of after) if (!was.has(name)) this.#note(added, name);
    for (const name of before) if (!now.has(name)) this.#note(removed, name);
  }

  /** Keeps the wall's order: a ticker that changed goes first, one that didn't stays */
  #wall(tickers: Schemas["RoomTicker"][]) {
    const held = new Map(this.wall.map((l) => [l.username, l]));
    const now = Date.now();
    const fresh: WallLine[] = [];
    const kept: WallLine[] = [];
    for (const { username, ticker, hidden } of tickers) {
      const line = held.get(username);
      if (line && line.ticker === ticker)
        kept.push(line.hidden === hidden ? line : { ...line, hidden });
      else fresh.push({ username, ticker, hidden, at: line || this.wall.length ? now : 0 });
    }
    this.wall = [...fresh, ...kept].toSorted((a, b) => b.at - a.at);
  }

  #note(kind: PresenceKind, username: string) {
    addChange(this.bursts, { id: ++seq, at: new Date().toISOString(), kind, username });
    if (this.bursts.length > KEPT) this.bursts.splice(0, this.bursts.length - KEPT);
  }
}
