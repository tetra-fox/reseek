import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { session } from "#lib/session.svelte.js";
import { sharedFile } from "#lib/testing/fixtures.js";
import { fakeNetwork } from "#lib/testing/network.js";

import FolderFiles from "./FolderFiles.svelte";

describe("every file in a search result's folder", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    session.principal = null;
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  /** Opens the dialog on a two-folder album found by the search "s1" */
  const open = () => {
    session.principal = { name: "me", role: "Administrator" } as never;
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/transfers/downloads/jobs") ? { id: "j", groups: [] } : undefined
    );
    component = mount(FolderFiles, {
      target: document.body,
      props: {
        open: true,
        username: "alice",
        folder: "@@a\\Album",
        files: [sharedFile("@@a\\Album\\cover.jpg"), sharedFile("@@a\\Album\\CD1\\01.flac")],
        connected: true,
        searchId: "s1"
      }
    });
    flushSync();
    return net;
  };
  const button = (text: string) =>
    [...document.querySelectorAll("button")].find((b) => b.textContent?.trim() === text)!;
  /** each group of the one job asked for, as its search and its destination */
  const asked = async (net: ReturnType<typeof fakeNetwork>) => {
    await vi.waitFor(() => expect(net.requests).toHaveLength(1));
    const body = net.requests[0].body as {
      groups: { searchId: string | null; destination: string | null }[];
    };
    return body.groups.map((g) => [g.searchId, g.destination]);
  };

  it("downloads them as found by the search, each folder in its place", async () => {
    const net = open();
    button("Download all").click();
    expect(await asked(net)).toEqual([
      ["s1", "Album"],
      ["s1", "Album/CD1"]
    ]);
  });

  it("downloads a selection as found by the search too", async () => {
    const net = open();
    document.querySelector<HTMLElement>("[aria-label='Select 01.flac']")!.click();
    flushSync();
    button("Download").click();
    expect(await asked(net)).toEqual([["s1", null]]);
  });
});
