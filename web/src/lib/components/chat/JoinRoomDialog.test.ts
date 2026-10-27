import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { chat } from "#lib/chat/chat.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import JoinRoomDialog from "./JoinRoomDialog.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const flush = async () => {
  for (let i = 0; i < 3; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
};
const room = (name: string, userCount: number) => ({
  name,
  userCount,
  isPrivate: false,
  isOwned: false,
  isModerated: false,
  joined: false
});

describe("the join dialog", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    chat.clear();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("lists the busiest rooms first, picks one by click and joins it as private", async () => {
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/rooms/available")) {
        return [room("jazz", 4), room("indie", 40), room("ambient", 4)];
      }
      if (method === "POST") {
        return { status: 403, body: { error: "A private room with this name already exists" } };
      }
      return undefined;
    });
    component = mount(JoinRoomDialog, { target: document.body, props: { open: true } });
    await flush();
    const names = () =>
      [...document.querySelectorAll("[data-line]")].map((l) => l.getAttribute("data-line"));
    expect(names()).toEqual(["indie", "ambient", "jazz"]);

    document
      .querySelector('[data-line="jazz"]')!
      .dispatchEvent(new MouseEvent("click", { bubbles: true }));
    flushSync();
    expect(document.querySelector<HTMLInputElement>("#join-name")!.value).toBe("jazz");
    [...document.querySelectorAll("label")]
      .find((l) => l.textContent?.includes("Private"))!
      .querySelector<HTMLElement>("[role=checkbox]")!
      .click();
    flushSync();
    [...document.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Join")!.click();
    await flush();
    expect(net.requests.find((r) => r.method === "POST")?.body).toEqual({
      name: "jazz",
      private: true
    });
    // the server's refusal shows in the dialog, which stays open
    expect(document.body.textContent).toContain("A private room with this name already exists");
  });
});
