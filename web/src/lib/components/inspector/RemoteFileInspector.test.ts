import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { toasts } from "#lib/feedback/toast.svelte.js";
import { inspector } from "#lib/inspector/inspector.svelte.js";
import { session } from "#lib/session.svelte.js";
import { transfer } from "#lib/testing/fixtures.js";
import { press } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { downloads } from "#lib/transfers/transfers.svelte.js";

import RemoteFileInspector from "./RemoteFileInspector.svelte";

describe("a remote file in the inspector", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    session.principal = null;
    inspector.focusList = () => {};
    downloads.clear();
    toasts.splice(0);
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  /** Lets the menu open or close, short of the Undo's countdown */
  const settle = async () => {
    await vi.advanceTimersByTimeAsync(100);
    flushSync();
  };

  it("gives focus to the list when its menu removes the download it shows", async () => {
    vi.useFakeTimers();
    fakeNetwork(() => undefined);
    session.principal = {
      name: "a",
      role: "Administrator",
      session: null
    } as unknown as typeof session.principal;
    const t = transfer();
    downloads.list.upsert(t);
    const list = document.createElement("div");
    list.tabIndex = 0;
    document.body.append(list);
    inspector.focusList = () => list.focus();
    const file = { filename: t.filename, size: t.size } as Schemas["File"];
    component = mount(RemoteFileInspector, {
      target: document.body,
      props: { file, username: t.username }
    });
    flushSync();

    const more = document.querySelector<HTMLElement>('[aria-label="More actions"]')!;
    more.focus();
    press(more, "Enter");
    await settle();
    const remove = [...document.querySelectorAll<HTMLElement>("[role=menuitem]")].find((i) =>
      i.textContent?.includes("Remove")
    )!;
    remove.focus();
    press(remove, "Enter");
    await settle();
    // the transfer's header and its menu's trigger left with it; the file's own came instead
    expect(document.querySelector("h2")?.textContent).toBe("01 One.flac");
    expect(document.activeElement).toBe(list);
  });
});
