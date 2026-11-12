import { activity } from "#lib/activity/activity.svelte.js";
import { jobs as analysisJobs } from "#lib/analysis/jobs.svelte.js";
import { marks } from "#lib/analysis/marks.svelte.js";
import { api, onUnauthorized, type Schemas } from "#lib/api/client.js";
import { browser } from "#lib/browse/browse.svelte.js";
import { chat } from "#lib/chat/chat.svelte.js";
import { withValueAt } from "#lib/config/sections.js";
import { toast } from "#lib/feedback/toast.svelte.js";
import { jobs } from "#lib/jobs/jobs.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";
import { searches } from "#lib/search/searches.svelte.js";
import { wishlist } from "#lib/search/wishlist.svelte.js";
import { session } from "#lib/session.svelte.js";
import { downloads, uploads } from "#lib/transfers/transfers.svelte.js";
import { unseen } from "#lib/transfers/unseen.svelte.js";
import { waits } from "#lib/transfers/why.svelte.js";
import { browserTabs } from "#lib/ui/tabs.js";
import { ownProfile } from "#lib/users/profile.svelte.js";
import { users } from "#lib/users/users.svelte.js";

export type AppState = Schemas["State"];
export type Metrics = Schemas["TransferMetrics"];
/** The options in effect, by their configuration keys, secrets hidden */
export type Options = Schemas["OptionsDocument"]["values"];

/**
 * Where the shell is: checking the session, showing the login, showing the app, or showing the
 * full-page message because nothing could be loaded at all
 */
export type Phase = "initializing" | "login" | "app" | "lost";

/** Why the app could not start when reseek did not answer */
const LOST = "Lost connection to reseek";

/**
 * The application as the shell shows it: the daemon's state document, the options in effect
 * and the transfer figures, each replaced whole by its frame, and the bootstrap that gets there
 */
class App {
  state = $state.raw<AppState | null>(null);
  /** the options in effect, by their configuration keys */
  options = $state.raw<Options | null>(null);
  /** the revision of `options`, which a change made from them names */
  optionsRevision: string | null = null;
  metrics = $state.raw<Metrics | null>(null);
  phase = $state<Phase>("initializing");
  /** why the last bootstrap ended in `lost` */
  lostReason = $state("");
  #release: (() => void)[] = [];
  /**
   * counts the bootstraps started and the times the phase was taken from one, by a sign-out or
   * the shell closing; a bootstrap that is no longer the last one leaves the phase alone
   */
  #boot = 0;

  constructor() {
    live.on("application.state", (data) => (this.state = data));
    live.on("application.options", (document) => this.#tookOptions(document));
    live.on("metrics.update", (data) => (this.metrics = data));
    // a broadcast from the Soulseek server's administrators, or a notice to us
    live.on("application.server_message", ({ message }) =>
      toast(`Soulseek server: ${message}`, "info", { ms: 30_000 })
    );
    onUnauthorized(() => this.signedOut());
    // a tab's focus counts toward playing the sounds only while its stream brings the frames
    $effect.root(() => {
      $effect(() => {
        browserTabs()?.connected(live.status === "connected");
      });
    });
    session.onEnded(() => this.signedOut());
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
    const boot = ++this.#boot;
    this.phase = "initializing";
    // the tabs following the stream agree on the one that plays a sound; this one joins now, so
    // the wait for their answers overlaps the session check
    browserTabs()?.join();
    const found = await session.check();
    if (boot !== this.#boot) return;
    if (found === "out") {
      browserTabs()?.quit();
      this.phase = "login";
      return;
    }
    if (found === "unreachable") return this.#lost(LOST);
    // the stream opens once the tabs have had their answers, so its first frame finds one leading
    const settling = browserTabs()?.settling() ?? 0;
    if (settling) await new Promise((done) => setTimeout(done, settling));
    if (boot !== this.#boot) return;
    this.#follow();
    const missing = await this.#firstState();
    if (boot !== this.#boot) return;
    if (missing) return this.#lost(missing);
    this.phase = "app";
  };

  /**
   * Fetches the state document, the options in effect and the transfer figures, for when frames
   * don't arrive. Gives back what could not be had of the state and the options, which the app
   * needs to show, or null
   */
  refresh = async (): Promise<string | null> => {
    const [state, options, metrics] = await Promise.all([
      call(() => api.GET("/api/v0/application")),
      call(() => api.GET("/api/v0/options")),
      call(() => api.GET("/api/v0/telemetry/transfers"))
    ]);
    if (state.ok) this.state = state.data;
    if (options.ok) this.#tookOptions(options.data);
    // figures that can't be read leave the last ones shown until the next poll
    if (metrics.ok) this.metrics = metrics.data;
    return this.#missing();
  };

  /** What of the state and the options is not here yet, null once both are */
  #missing(): string | null {
    if (!this.state) return LOST;
    if (!this.options) return "Could not load the settings";
    return null;
  }

  #tookOptions(document: Schemas["OptionsDocument"]) {
    this.options = document.values;
    this.optionsRevision = document.revision;
  }

  /**
   * Takes the values `set`, by their keys, that a write of ours made against revision `from` left
   * in effect at `to`, ahead of the frame that brings them, so a change made right after it is
   * made from them. Once the options are at another revision than `from`, a frame brought this
   * write or a later one already, and they stay as they are
   */
  wrote(set: Record<string, unknown>, from: string, to: string) {
    if (!this.options || this.optionsRevision !== from) return;
    let values = this.options;
    for (const [key, value] of Object.entries(set))
      values = withValueAt(values, key, value) as Options;
    this.options = values;
    this.optionsRevision = to;
  }

  /**
   * Reads the options again, for a tab whose change was refused as made against old ones. A frame
   * that arrives meanwhile is at least as new, so the answer is then left
   */
  readOptions = async () => {
    const held = this.optionsRevision;
    const outcome = await call(() => api.GET("/api/v0/options"));
    if (outcome.ok && this.optionsRevision === held) this.#tookOptions(outcome.data);
  };

  /** The session is gone: everything kept for it goes, and the login shows */
  signedOut() {
    this.#boot += 1;
    browserTabs()?.quit();
    for (const release of this.#release.splice(0)) release();
    live.stop();
    session.clear();
    users.clear();
    chat.clear();
    searches.clear();
    wishlist.clear();
    downloads.clear();
    uploads.clear();
    unseen.clear();
    waits.clear();
    jobs.clear();
    analysisJobs.clear();
    marks.clear();
    activity.clear();
    browser.clear();
    ownProfile.clear();
    this.state = null;
    this.options = null;
    this.optionsRevision = null;
    this.metrics = null;
    this.phase = "login";
  }

  /**
   * The shell is gone: a bootstrap still running is abandoned and the stream closes, so the next
   * shell starts from checking the session, whatever phase this one ended in
   */
  close() {
    this.#boot += 1;
    browserTabs()?.quit();
    live.stop();
    this.phase = "initializing";
  }

  #follow() {
    if (!this.#release.length) {
      // jobs are followed on every screen, for their notifications
      this.#release.push(live.retain("application"), live.retain("metrics"), live.retain("jobs"));
    }
    live.start();
  }

  /**
   * Resolves once the state document and the options are here, from the stream or fetched when
   * they can't come, with null; else with what could not be had
   */
  async #firstState(): Promise<string | null> {
    if (!this.#missing()) return null;
    if (live.status === "connecting") {
      await new Promise<void>((resolve) => {
        const stops: (() => void)[] = [];
        const done = () => {
          for (const stop of stops.splice(0)) stop();
          resolve();
        };
        const both = () => {
          if (!this.#missing()) done();
        };
        // the stream's snapshots bring the state, then the options
        stops.push(live.on("application.state", both), live.on("application.options", both));
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
    return this.#missing() && (await this.refresh());
  }

  #lost(reason: string) {
    browserTabs()?.quit();
    this.lostReason = reason;
    this.phase = "lost";
  }
}

export const app = new App();
