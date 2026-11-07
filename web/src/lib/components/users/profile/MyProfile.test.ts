import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";
import fixture from "#lib/config/fixtures/view.json";
import { session } from "#lib/session.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { ownProfile } from "#lib/users/profile.svelte.js";

import MyProfile from "./MyProfile.svelte";

vi.mock("$app/navigation", () => ({ beforeNavigate: () => {}, goto: vi.fn() }));

const file = { layer: "file", file: "config.toml" };

const own = {
  description: "hello",
  hasPicture: false,
  pictureUrl: null,
  uploadSlots: 2,
  freeUploadSlot: true,
  queueLength: 0,
  interests: { liked: ["jazz", "dub"], hated: ["polka"] },
  sources: { description: file, picture: { layer: "default" }, liked: file, hated: file }
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

  /** Signs in as an administrator who may edit the profile, and gives the view that says so */
  const asAdmin = () => {
    session.principal = { name: "me", role: "Administrator" } as never;
    app.state = {
      user: {
        username: "me",
        statistics: { averageSpeed: 0, directoryCount: 2, fileCount: 30, uploadCount: 0 }
      },
      server: { isLoggedIn: true }
    } as never;
    const view = structuredClone(fixture) as typeof fixture;
    (view.values as Record<string, unknown>).remote_configuration = true;
    for (const file of view.files) file.writable = true;
    vi.stubGlobal(
      "URL",
      Object.assign(URL, { createObjectURL: () => "blob:picked", revokeObjectURL: () => {} })
    );
    return view;
  };
  const type = (text: string) => {
    const textarea = document.querySelector<HTMLTextAreaElement>("#profile-text")!;
    textarea.value = text;
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  };
  const pickPicture = () => {
    const file = new File([new Uint8Array([0xff, 0xd8, 0xff])], "me.jpg", { type: "image/jpeg" });
    const input = document.querySelector<HTMLInputElement>('input[type="file"]')!;
    Object.defineProperty(input, "files", { value: [file], configurable: true });
    input.dispatchEvent(new Event("change", { bubbles: true }));
    flushSync();
  };
  const saveButton = () =>
    [...document.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Save")!;

  it("previews a picked picture and changed text, and saves both", async () => {
    const view = asAdmin();
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/api/v0/options/view")) return view;
      if (url.pathname.endsWith("/api/v0/profile") && method === "GET") return own;
      if (method === "PUT") return { status: 204 };
      return undefined;
    });
    component = mount(MyProfile, { target: document.body });
    await flush();

    expect(document.querySelector<HTMLTextAreaElement>("#profile-text")!.value).toBe("hello");
    type("plays bass\nlikes dub");
    pickPicture();

    // the preview and the card follow the draft before anything is saved
    expect([...document.querySelectorAll("img")].map((i) => i.getAttribute("src"))).toContain(
      "blob:picked"
    );
    expect(document.body.textContent).toContain("plays bass");
    // the card shows our own presence and share counts, not the directory's unknowns
    expect(document.body.textContent).toContain("Online");
    expect(document.body.textContent).toContain("30 files");
    expect(net.requests.filter((r) => r.method === "PUT")).toEqual([]);

    saveButton().click();
    await flush();
    const puts = net.requests.filter((r) => r.method === "PUT");
    expect(puts.map((r) => r.url.pathname)).toEqual(["/api/v0/profile/picture", "/api/v0/profile"]);
    expect(puts[1].body).toEqual({ description: "plays bass\nlikes dub" });
  });

  it("saves once, and keeps text typed while it saves as a change", async () => {
    const view = asAdmin();
    let stored = own;
    let reread: Promise<void> | null = null;
    const net = fakeNetwork(async (url, method, body) => {
      if (url.pathname.endsWith("/api/v0/options/view")) return view;
      if (url.pathname.endsWith("/api/v0/profile") && method === "GET") {
        if (reread) await reread;
        return stored;
      }
      if (url.pathname.endsWith("/api/v0/profile")) {
        stored = { ...stored, ...(body as object) };
        return { status: 204 };
      }
      if (method === "PUT") return { status: 204 };
      return undefined;
    });
    component = mount(MyProfile, { target: document.body });
    await flush();
    let release!: () => void;
    reread = new Promise((done) => (release = done));
    type("plays bass");
    pickPicture();
    saveButton().click();
    await flush();
    // the profile is read again after the writes, and the save is not over until it is
    expect(saveButton().disabled).toBe(true);
    saveButton().click();
    type("plays bass and drums");
    release();
    await flush();
    const puts = net.requests.filter((r) => r.method === "PUT");
    expect(puts.map((r) => r.url.pathname)).toEqual(["/api/v0/profile/picture", "/api/v0/profile"]);
    expect(saveButton().disabled).toBe(false);
  });

  it("sends again only what failed to save", async () => {
    const view = asAdmin();
    let refuse = true;
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/api/v0/options/view")) return view;
      if (url.pathname.endsWith("/api/v0/profile") && method === "GET") return own;
      if (url.pathname.endsWith("/api/v0/profile") && refuse)
        return { status: 409, body: { error: "The configuration file changed on disk" } };
      if (method === "PUT") return { status: 204 };
      return undefined;
    });
    component = mount(MyProfile, { target: document.body });
    await flush();
    type("plays the tuba");
    pickPicture();
    saveButton().click();
    await flush();
    expect(document.body.textContent).toContain("The configuration file changed on disk");

    refuse = false;
    saveButton().click();
    await flush();
    const puts = net.requests.filter((r) => r.method === "PUT").map((r) => r.url.pathname);
    expect(puts).toEqual(["/api/v0/profile/picture", "/api/v0/profile", "/api/v0/profile"]);
  });

  it("saves interests as they change, each on one list only", async () => {
    const view = asAdmin();
    let stored = own;
    const net = fakeNetwork((url, method, body) => {
      if (url.pathname.endsWith("/api/v0/options/view")) return view;
      if (url.pathname.endsWith("/api/v0/profile") && method === "GET") return stored;
      if (url.pathname.endsWith("/api/v0/profile/interests")) {
        stored = { ...stored, interests: body as typeof own.interests };
        return { status: 204 };
      }
      return undefined;
    });
    component = mount(MyProfile, { target: document.body });
    await flush();

    const dislike = document.querySelector<HTMLInputElement>('[aria-label="Add a dislike"]')!;
    dislike.value = " jazz ";
    dislike.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    dislike.closest("form")!.requestSubmit();
    await flush();
    const puts = net.requests.filter((r) => r.method === "PUT");
    expect(puts.map((r) => r.body)).toEqual([{ liked: ["dub"], hated: ["polka", "jazz"] }]);
    expect(dislike.value).toBe("");
    expect(document.querySelector('[aria-label="Remove jazz"]')).not.toBeNull();

    // focus stays in the list a chip is taken off, and goes to its box once the list is empty
    document.querySelector<HTMLButtonElement>('[aria-label="Remove polka"]')!.click();
    await flush();
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Remove jazz");
    document.querySelector<HTMLButtonElement>('[aria-label="Remove jazz"]')!.click();
    await flush();
    expect(document.activeElement).toBe(dislike);
  });

  it("shows the interests without a way to change them while remote configuration is off", async () => {
    asAdmin();
    fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/api/v0/options/view")) return fixture;
      if (url.pathname.endsWith("/api/v0/profile") && method === "GET") return own;
      return undefined;
    });
    // the profile is fetched once for the page and kept, so it still holds what the tests
    // before saved
    await ownProfile.loader.load();
    component = mount(MyProfile, { target: document.body });
    await flush();
    const liked = document.querySelector('[aria-label="Interests"]');
    expect(liked?.textContent).toContain("jazz");
    expect(document.querySelector('[aria-label="Remove jazz"]')).toBeNull();
    expect(document.querySelector('[aria-label="Add a dislike"]')).toBeNull();
  });
});
