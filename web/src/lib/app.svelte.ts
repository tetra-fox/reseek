import { api, onUnauthorized, type Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";
import { session } from "#lib/session.svelte.js";
import { toast } from "#lib/toast.svelte.js";
import { users } from "#lib/users.svelte.js";

export type AppState = Schemas["State"];
export type Metrics = Schemas["TransferMetrics"];

/**
 * Where the shell is: checking the session, showing the login, showing the app, or showing the
 * full-page message because nothing could be loaded at all
 */
export type Phase = "initializing" | "login" | "app" | "lost";

/**
 * The application as the shell shows it: the daemon's state document, the options in effect
 * and the transfer figures, each replaced whole by its frame, and the bootstrap that gets there
 */
class App {
  state = $state.raw<AppState | null>(null);
  options = $state.raw<Schemas["OptionsDocument"] | null>(null);
  metrics = $state.raw<Metrics | null>(null);
  phase = $state<Phase>("initializing");
  /** why the last bootstrap ended in `lost` */
  lostReason = $state("");
  #release: (() => void)[] = [];

  constructor() {
    live.on("application.state", (data) => (this.state = data));
    live.on("application.options", (data) => (this.options = data));
    live.on("metrics.update", (data) => (this.metrics = data));
    // a broadcast from the Soulseek server's administrators, or a notice to us
    live.on("application.server_message", ({ message }) =>
      toast(`Soulseek server: ${message}`, "info", { ms: 30_000 })
    );
    onUnauthorized(() => this.signedOut());
    session.onEnded(() => this.signedOut());
  }

  /** "agent" when this daemon is a relay agent of another, which shows only the System area */
  get mode(): "normal" | "agent" {
    return this.state?.relay.mode === "Agent" ? "agent" : "normal";
  }

  get isServerConnected(): boolean {
    return this.state?.server.isConnected ?? false;
  }

  /** a change waits on a reconnect, a restart or a share scan */
  get pendingAction(): boolean {
    const s = this.state;
    return !!s && (s.pendingReconnect || s.pendingRestart || s.shares.scanPending);
  }

  /** Checks the session, then opens the stream and waits for the first state before the app shows */
  bootstrap = async () => {
    this.phase = "initializing";
    const found = await session.check();
    if (found === "out") {
      this.phase = "login";
      return;
    }
    if (found === "unreachable") return this.#lost("Lost connection to reseek");
    this.#follow();
    if (!(await this.#firstState())) return this.#lost("Lost connection to reseek");
    this.phase = "app";
  };

  /** Fetches the state document, for when frames don't arrive */
  refresh = async () => {
    const outcome = await call(() => api.GET("/api/v0/application"));
    if (outcome.ok) this.state = outcome.data;
    return outcome.ok;
  };

  /** The session is gone: everything kept for it goes, and the login shows */
  signedOut() {
    for (const release of this.#release.splice(0)) release();
    live.stop();
    session.clear();
    users.clear();
    this.state = null;
    this.options = null;
    this.metrics = null;
    this.phase = "login";
  }

  #follow() {
    if (!this.#release.length) {
      this.#release.push(live.retain("application"), live.retain("metrics"));
    }
    live.start();
  }

  /** Resolves true once a state document is here: from the stream, or fetched when it can't come */
  async #firstState(): Promise<boolean> {
    if (this.state) return true;
    if (live.status === "connecting") {
      await new Promise<void>((resolve) => {
        const stops: (() => void)[] = [];
        const done = () => {
          for (const stop of stops.splice(0)) stop();
          resolve();
        };
        stops.push(live.on("application.state", done));
        // the stream ended before its first state: polling takes over
        stops.push(
          $effect.root(() => {
            $effect(() => {
              if (live.status !== "connecting" && live.status !== "connected") done();
            });
          })
        );
      });
    }
    return !!this.state || (await this.refresh());
  }

  #lost(reason: string) {
    this.lostReason = reason;
    this.phase = "lost";
  }
}

export const app = new App();
