import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ConfigView } from "#lib/config/fields.js";
import fixture from "#lib/config/fixtures/view.json";
import { session } from "#lib/session.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import Shares from "./Shares.svelte";

/** A share of the music directory, seen by `visibleTo` */
const music = (visibleTo: string[] | null) => ({
  alias: "Music",
  id: "1",
  isExcluded: false,
  localPath: "/srv/music",
  raw: "[Music]/srv/music",
  remotePath: "Music",
  visibleTo
});

describe("the shares page", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    session.principal = null;
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("takes a second change of who sees a share from the options the first left", async () => {
    session.principal = { name: "me", role: "Administrator" } as never;
    let options = structuredClone(fixture) as unknown as ConfigView;
    (options.values as { remote_configuration: boolean }).remote_configuration = true;
    (
      options.values as { transfers: { groups: { user_defined: object } } }
    ).transfers.groups.user_defined = { indie: {} };
    // the read of the options after a write, held until the test lets it answer
    let reread: (() => void) | null = null;
    let writes = 0;
    const net = fakeNetwork(async (url, method, body) => {
      if (url.pathname.endsWith("/options/view")) {
        if (writes > 0 && reread === null) await new Promise<void>((done) => (reread = done));
        return structuredClone(options);
      }
      if (url.pathname.endsWith("/api/v0/shares")) {
        const seen = (options.values as { shares: { visible_to: Record<string, string[]> } }).shares
          .visible_to.Music;
        return { local: [music(seen ?? null)] };
      }
      if (method !== "PATCH") return undefined;
      writes += 1;
      const set = (body as { set: Record<string, unknown> }).set;
      options = structuredClone(options);
      options.revision = `changed ${writes}`;
      (options.values as { shares: { visible_to: unknown } }).shares.visible_to =
        set["shares.visible_to"];
      return {
        revision: "file",
        optionsRevision: options.revision,
        applied: [],
        restartRequired: [],
        reconnectRequired: []
      };
    });
    component = mount(Shares, { target: document.body });
    await flush();
    await flush();
    const trigger = () =>
      document.querySelector<HTMLButtonElement>('button[aria-label^="Who sees Music"]')!;
    const choose = async (name: string) => {
      trigger().dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0 }));
      await flush();
      [...document.querySelectorAll<HTMLElement>("[role=menuitemcheckbox]")]
        .find((i) => i.textContent?.trim() === name)!
        .click();
      await flush();
    };
    await choose("buddies");
    await vi.waitFor(() => expect(reread).not.toBeNull());
    expect(trigger().disabled, "held until the options are read again").toBe(true);
    reread!();
    await vi.waitFor(() => expect(trigger().disabled).toBe(false));
    await choose("indie");
    await vi.waitFor(() => expect(writes).toBe(2));
    const sent = net.requests.filter((r) => r.method === "PATCH").map((r) => r.body);
    expect(sent[1]).toMatchObject({
      set: { "shares.visible_to": { Music: ["buddies", "indie"] } }
    });
  });
});
