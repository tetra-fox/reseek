import { api, type Schemas } from "#lib/api/client.js";
import { toast } from "#lib/feedback/toast.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";

export type ChatPin = Schemas["ChatPin"];

/** Whether two pins are the same chat */
export const samePin = (a: ChatPin, b: ChatPin) => a.kind === b.kind && a.name === b.name;

/**
 * The pinned chats in the user's order, kept by the daemon so they follow the user across browsers.
 * A change shows here at once. Writes go one at a time and only the newest list, so a run of
 * moves cannot reach the daemon out of order, and until they end this tab's list is the newest
 */
class Pins {
  /** every pin, those of a room left or a conversation closed too, which the list leaves out */
  list = $state.raw<ChatPin[]>([]);
  /** the list to send once the write under way ends */
  #next: ChatPin[] | null = null;
  #writing = false;

  constructor() {
    live.on("chat.pins", (list) => {
      if (!this.#writing) this.list = list;
    });
  }

  /** Whether `chat` is pinned */
  has = (chat: ChatPin) => this.list.some((pin) => samePin(pin, chat));

  /** Reads the pins. One that fails leaves what is held; the stream's snapshot brings them too */
  read = async () => {
    const outcome = await call(() => api.GET("/api/v0/chat/pins"));
    if (outcome.ok && !this.#writing) this.list = outcome.data;
  };

  /** Pins `chats` after the others, or takes their pins off */
  set = (chats: ChatPin[], on: boolean) => {
    const fresh = chats.filter((chat) => !this.has(chat));
    const others = this.list.filter((pin) => !chats.some((chat) => samePin(pin, chat)));
    void this.#write(on ? [...this.list, ...fresh] : others);
  };

  /** Moves a pin in front of `before`, or last without one */
  move = (pin: ChatPin, before: ChatPin | null) => {
    if (!this.has(pin)) return;
    const others = this.list.filter((other) => !samePin(other, pin));
    const at = before ? others.findIndex((other) => samePin(other, before)) : -1;
    others.splice(at === -1 ? others.length : at, 0, pin);
    void this.#write(others);
  };

  /** Forgets the pins held for the session that ended */
  clear = () => {
    this.list = [];
    this.#next = null;
  };

  #write = async (list: ChatPin[]) => {
    this.list = list;
    this.#next = list;
    if (this.#writing) return;
    this.#writing = true;
    while (this.#next) {
      const body = this.#next;
      this.#next = null;
      const outcome = await call(() => api.PUT("/api/v0/chat/pins", { body }));
      if (!outcome.ok) {
        toast(outcome.error, "error");
        this.#next = null;
        this.#writing = false;
        return this.read();
      }
    }
    this.#writing = false;
  };
}

export const pins = new Pins();
