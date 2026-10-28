import { api, byUser, type Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { plural } from "#lib/format.js";
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
 * Lifts the block on `username`, giving back the request that makes the same block again (its
 * reason, scopes and end), null when none was held. The failure is toasted
 */
const lift = async (username: string): Promise<{ ok: boolean; again: BlockRequest | null }> => {
  await users.load(username);
  const was = users.get(username)?.block;
  const outcome = await call(() =>
    api.DELETE("/api/v0/users/{username}/block", { params: byUser(username) })
  );
  if (!outcome.ok) {
    toast(`${username}: ${outcome.error}`, "error");
    return { ok: false, again: null };
  }
  await users.load(username, { fresh: true });
  const ends = was?.expiresAt && Date.parse(was.expiresAt) > Date.now() ? was.expiresAt : null;
  return {
    ok: true,
    again: was
      ? { reason: was.reason, scopes: was.scopes, expiresAt: ends, cancelTransfers: false }
      : null
  };
};

/** How long the toast after an unblock offers to undo it */
export const UNBLOCK_UNDO_MS = 5_000;

/**
 * Unblocks the users in `usernames` one after another, then says so in one toast whose Undo
 * makes each block again as it was
 */
export const unblockMany = async (usernames: string[]) => {
  const lifted: { username: string; again: BlockRequest }[] = [];
  let done = 0;
  for (const username of usernames) {
    const { ok, again } = await lift(username);
    if (ok) done += 1;
    if (again) lifted.push({ username, again });
  }
  if (!done) return;
  toast(
    done === 1 && usernames.length === 1
      ? `${usernames[0]} is unblocked`
      : `${plural(done, "user", "users")} unblocked`,
    "info",
    {
      ms: UNBLOCK_UNDO_MS,
      action: lifted.length
        ? {
            label: "Undo",
            run: () =>
              void (async () => {
                for (const { username, again } of lifted) {
                  const o = await block(username, again);
                  if (!o.ok) toast(`${username}: ${o.error}`, "error");
                }
              })()
          }
        : undefined
    }
  );
};

/** Unblocks `username` in one click; the toast offers to undo it for a few seconds */
export const unblock = (username: string) => unblockMany([username]);
