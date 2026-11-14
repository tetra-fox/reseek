import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";

import { app } from "#lib/app.svelte.js";
import type { ConfigView } from "#lib/config/fields.js";
import fixture from "#lib/config/fixtures/view.json";

import Overview from "./Overview.svelte";

const view = {
  ...structuredClone(fixture),
  pendingReconnect: ["soulseek.address"]
} as unknown as ConfigView;

const connected = (isConnected: boolean) =>
  ({
    server: { isConnected },
    pendingReconnect: false,
    pendingRestart: false,
    shares: { scanPending: false }
  }) as never;

describe("the settings overview", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    app.state = null;
    document.body.innerHTML = "";
  });

  it("asks for a reconnect only while connected", () => {
    app.state = connected(false);
    component = mount(Overview, {
      target: document.body,
      props: { view, onchanged: async () => {} }
    });
    flushSync();
    const notice = () => document.body.textContent!.includes("Reconnect to the Soulseek server");
    expect(notice()).toBe(false);
    app.state = connected(true);
    flushSync();
    expect(notice()).toBe(true);
  });
});
