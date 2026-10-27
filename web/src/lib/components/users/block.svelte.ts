import { api, byUser, type Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { call } from "#lib/request.js";
import { fields } from "#lib/stored.svelte.js";
import { toast } from "#lib/toast.svelte.js";
import { users } from "#lib/users.svelte.js";

export type BlockScope = Schemas["BlockScope"];
export type BlockRequest = Schemas["BlockRequest"];

/** What a block stops the user from, in the order the dialog lists it */
export const STOP_SCOPES: { scope: BlockScope; label: string }[] = [
  { scope: "uploads", label: "downloading from you" },
  { scope: "searches", label: "getting your search results" },
  { scope: "browse", label: "browsing your shares" },
  { scope: "messages", label: "messaging you" },
  { scope: "rooms", label: "being seen in your rooms" }
];

/** What a block may do besides, off unless chosen */
export const ALSO_SCOPES: { scope: BlockScope; label: string }[] = [
  { scope: "downloads", label: "cancel my downloads from them" },
  { scope: "distributed", label: "drop them from the distributed network" }
];

const ALL = [...STOP_SCOPES, ...ALSO_SCOPES].map((s) => s.scope);

/** The scopes a new block starts with: `blocking.default_scopes` from the options in effect */
export const defaultScopes = (): BlockScope[] => {
  const configured = fields(fields(app.options).blocking).default_scopes;
  return Array.isArray(configured)
    ? ALL.filter((scope) => configured.includes(scope))
    : STOP_SCOPES.map((s) => s.scope);
};

type Ask = {
  usernames: string[];
  /** scopes ticked besides the defaults, such as `uploads` for Deny and block */
  add?: BlockScope[];
};

/** The one block dialog of the page: who it asks about, while it is open */
class Blocking {
  asked = $state.raw<Ask | null>(null);
  open = $state(false);

  /** Opens the dialog for one user or several */
  ask(who: string | string[], add?: BlockScope[]) {
    this.asked = { usernames: typeof who === "string" ? [who] : who, add };
    this.open = true;
  }
}

export const blocking = new Blocking();

/** Blocks `username` as `request` says, then refreshes what the page knows of them */
export const block = async (username: string, request: BlockRequest) => {
  const outcome = await call(() =>
    api.PUT("/api/v0/users/{username}/block", { params: byUser(username), body: request })
  );
  if (outcome.ok) await users.load(username, { fresh: true });
  return outcome;
};

/**
 * Unblocks `username` in one click; the toast offers to undo it for five seconds, which makes
 * the block again with its reason, scopes and end
 */
export const unblock = async (username: string) => {
  await users.load(username);
  const was = users.get(username)?.block;
  const outcome = await call(() =>
    api.DELETE("/api/v0/users/{username}/block", { params: byUser(username) })
  );
  if (!outcome.ok) return void toast(outcome.error, "error");
  await users.load(username, { fresh: true });
  const ends = was?.expiresAt && Date.parse(was.expiresAt) > Date.now() ? was.expiresAt : null;
  const again: BlockRequest | null = was
    ? { reason: was.reason, scopes: was.scopes, expiresAt: ends, cancelTransfers: false }
    : null;
  toast(`${username} is unblocked`, "info", {
    ms: 5_000,
    action: again
      ? {
          label: "Undo",
          run: () => void block(username, again).then((o) => !o.ok && toast(o.error, "error"))
        }
      : undefined
  });
};
