import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MessagePane } from "#lib/chat/pane.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import Composer from "./Composer.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

describe("the composer", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  const open = () => {
    fakeNetwork(() => undefined);
    const pane = new MessagePane({ kind: "room", name: "indie" }, () => "me");
    component = mount(Composer, {
      target: document.body,
      props: { pane, disabled: null, names: () => ["alice"] }
    });
    flushSync();
    const box = document.querySelector<HTMLTextAreaElement>("textarea")!;
    const type = (text: string, at = text.length) => {
      box.value = text;
      box.setSelectionRange(at, at);
      box.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
    };
    const press = (key: string) => {
      box.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
      flushSync();
    };
    return { box, type, press };
  };

  it("puts an emoji in place of its name with Tab, the caret after it", async () => {
    const { box, type, press } = open();
    const errors: unknown[] = [];
    const caught = (e: ErrorEvent) => errors.push(e.error);
    window.addEventListener("error", caught);
    type("hi :smi there", "hi :smi".length);
    press("Tab");
    await Promise.resolve();
    window.removeEventListener("error", caught);
    expect(errors).toEqual([]);
    expect(box.value).toBe("hi 😄  there");
    expect(box.selectionStart).toBe("hi 😄 ".length);
  });

  it("runs a command typed in full on Enter, and completes a partial one", () => {
    const { box, type, press } = open();
    type("/hel");
    press("Enter");
    expect(box.value).toBe("/help ");
    type("/help");
    press("Enter");
    expect(document.querySelector('[role="dialog"][aria-label="Chat commands"]')).not.toBeNull();
    expect(box.value).toBe("");
  });

  it("quotes an action as what was done", async () => {
    const { box } = open();
    const quote = (component as unknown as { quote: (m: unknown) => void }).quote;
    quote({ author: "carol", text: "/me waves at everyone" });
    flushSync();
    expect(box.value).toBe("[carol] waves at everyone --> ");
  });
});
