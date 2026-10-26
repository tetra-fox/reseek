import { api, type Schemas } from "#lib/api/client.js";
import { call, type Outcome } from "#lib/request.js";

export type Role = Schemas["Role"];
export type Principal = Schemas["CurrentPrincipal"];

const RANK: Record<Role, number> = { ReadOnly: 0, ReadWrite: 1, Administrator: 2 };

/** After the tab was hidden this long, the session is checked again when it comes back */
export const IDLE_CHECK_MS = 5 * 60_000;

/** What a session check found */
export type Check = "in" | "out" | "unreachable";

/**
 * Who is signed in. The session itself is a cookie the browser keeps; this holds what
 * `GET /session` said about it, checks it again when it is due to expire and when the tab comes
 * back after a while, and says so through `onEnded` when it is gone
 */
class Session {
  principal = $state.raw<Principal | null>(null);
  #expiry: ReturnType<typeof setTimeout> | undefined;
  #hiddenAt: number | null = null;
  #ended: () => void = () => {};

  get role(): Role | null {
    return this.principal?.role ?? null;
  }

  /** Whether the signed-in principal has at least `role` */
  can(role: Role): boolean {
    return this.role !== null && RANK[this.role] >= RANK[role];
  }

  /** A browser session can be signed out of; a trusted network or a token can't */
  get canSignOut(): boolean {
    return !!this.principal?.session;
  }

  /** Called when a check finds the session gone after it was in */
  onEnded(handler: () => void) {
    this.#ended = handler;
  }

  /**
   * Asks the daemon whether this browser is signed in, and as whom. A browser let in without a
   * login (a trusted network, or authentication off) is answered with a principal that has no
   * session, which is all `canSignOut` needs to know
   */
  check = async (): Promise<Check> => {
    const current = await call(() => api.GET("/api/v0/session"));
    if (current.ok) {
      this.#set(current.data);
      return "in";
    }
    if (current.status === 401) {
      this.clear();
      return "out";
    }
    return "unreachable";
  };

  /** Password login; the daemon sets the session cookie */
  login = async (username: string, password: string): Promise<Outcome<unknown>> => {
    const outcome = await call(() => api.POST("/api/v0/session", { body: { username, password } }));
    // a 403 says why in the daemon's words (password login off, not from this address, headless)
    if (!outcome.ok && outcome.status === 401) {
      return { ...outcome, error: "Invalid username or password" };
    }
    return outcome;
  };

  /** Ends the session; the identity provider's logout page when it has one, else null */
  logout = async (): Promise<Outcome<string | null>> => {
    const outcome = await call(() => api.POST("/api/v0/session/logout"));
    if (!outcome.ok) return outcome;
    this.clear();
    return { ok: true, data: outcome.data?.redirect ?? null };
  };

  /** Forgets the principal, as when the session ended */
  clear() {
    clearTimeout(this.#expiry);
    this.principal = null;
  }

  /** The tab was hidden or shown; a long absence checks the session again */
  visibility = (hidden: boolean, now = Date.now()) => {
    if (hidden) {
      this.#hiddenAt = now;
      return;
    }
    const away = this.#hiddenAt === null ? 0 : now - this.#hiddenAt;
    this.#hiddenAt = null;
    if (away > IDLE_CHECK_MS && this.principal) void this.#recheck();
  };

  #set(principal: Principal) {
    this.principal = principal;
    clearTimeout(this.#expiry);
    const expires = principal.session?.expiresAt;
    if (!expires) return;
    // timers fire at most 2^31 - 1 ms out, about 24 days; a later expiry is checked on the way
    const ms = Math.min(Math.max(Date.parse(expires) - Date.now(), 0) + 1_000, 2 ** 31 - 1);
    this.#expiry = setTimeout(() => void this.#recheck(), ms);
  }

  async #recheck() {
    const found = await this.check();
    if (found === "out") this.#ended();
  }
}

export const session = new Session();
