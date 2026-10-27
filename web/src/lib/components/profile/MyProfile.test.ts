import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";
import fixture from "#lib/config/fixtures/view.json";
import { session } from "#lib/session.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import MyProfile from "./MyProfile.svelte";

vi.mock("$app/navigation", () => ({ beforeNavigate: () => {}, goto: vi.fn() }));

const flush = async () => {
  for (let i = 0; i < 3; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
};

const own = {
  description: "hello",
  hasPicture: false,
  pictureUrl: null,
  uploadSlots: 2,
  freeUploadSlot: true,
  queueLength: 0,
  sources: { description: { layer: "file", file: "config.toml" }, picture: { layer: "default" } }
};

describe("your own profile page", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    session.principal = null;
    app.state = null;
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("previews a picked picture and changed text, and saves both", async () => {
    session.principal = { name: "me", role: "Administrator" } as never;
    app.state = { user: { username: "me" } } as never;
    const view = structuredClone(fixture) as typeof fixture;
    (view.values as Record<string, unknown>).remote_configuration = true;
    for (const file of view.files) file.writable = true;
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/api/v0/options/view")) return view;
      if (url.pathname.endsWith("/api/v0/profile") && method === "GET") return own;
      if (method === "PUT") return { status: 204 };
      return undefined;
    });
    vi.stubGlobal(
      "URL",
      Object.assign(URL, { createObjectURL: () => "blob:picked", revokeObjectURL: () => {} })
    );
    component = mount(MyProfile, { target: document.body });
    await flush();

    const textarea = document.querySelector<HTMLTextAreaElement>("#profile-text")!;
    expect(textarea.value).toBe("hello");
    textarea.value = "plays bass\nlikes dub";
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
    const file = new File([new Uint8Array([0xff, 0xd8, 0xff])], "me.jpg", { type: "image/jpeg" });
    const input = document.querySelector<HTMLInputElement>('input[type="file"]')!;
    Object.defineProperty(input, "files", { value: [file], configurable: true });
    input.dispatchEvent(new Event("change", { bubbles: true }));
    flushSync();

    // the preview and the card follow the draft before anything is saved
    expect([...document.querySelectorAll("img")].map((i) => i.getAttribute("src"))).toContain(
      "blob:picked"
    );
    expect(document.body.textContent).toContain("plays bass");
    expect(net.requests.filter((r) => r.method === "PUT")).toEqual([]);

    [...document.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Save")!.click();
    await flush();
    const puts = net.requests.filter((r) => r.method === "PUT");
    expect(puts.map((r) => r.url.pathname)).toEqual(["/api/v0/profile/picture", "/api/v0/profile"]);
    expect(puts[1].body).toEqual({ description: "plays bass\nlikes dub" });
  });
});
