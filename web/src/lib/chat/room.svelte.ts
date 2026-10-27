import { api, byRoom, type Schemas } from "#lib/api/client.js";
import { Keyed } from "#lib/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";

export type RoomUser = Schemas["RoomUser"];

/**
 * Who is in a joined room and who runs it: read once, then kept by the room's frames, a member
 * joining or leaving patching the list in place
 */
export class RoomMembers {
  /** the room */
  readonly name: string;
  /** who is in it now, by username */
  readonly users = new Keyed<RoomUser>((u) => u.username);
  /** the room as last read or pushed: owner, operators, tickers */
  room = $state.raw<Schemas["Room"] | null>(null);
  /** why the last read failed, empty when it didn't */
  error = $state("");

  constructor(name: string) {
    this.name = name;
  }

  /** Follows the room's frames; call from a component's setup */
  follow() {
    $effect(() =>
      live.on("rooms.update", (room) => {
        if (room.name !== this.name) return;
        this.room = room;
        this.users.reconcile(room.users);
      })
    );
    $effect(() =>
      live.on("rooms.user_joined", ({ roomName, user }) => {
        if (roomName === this.name) this.users.upsert(user);
      })
    );
    $effect(() =>
      live.on("rooms.user_left", ({ roomName, username }) => {
        if (roomName === this.name) this.users.remove(username);
      })
    );
  }

  /** Reads the room and its users */
  load = async () => {
    const outcome = await call(() =>
      api.GET("/api/v0/rooms/joined/{roomName}", { params: byRoom(this.name) })
    );
    if (!outcome.ok) return void (this.error = outcome.error);
    this.error = "";
    this.room = outcome.data;
    this.users.reconcile(outcome.data.users);
  };

  /** The owner and operators of a private room */
  get operators(): Set<string> {
    const r = this.room;
    return new Set([...(r?.operators ?? []), ...(r?.owner ? [r.owner] : [])]);
  }
}
