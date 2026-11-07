import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MessagePane, type Target } from "#lib/chat/pane.svelte.js";
import { roomDrafts } from "#lib/chat/prefs.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import Composer from "./Composer.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

describe("the composer", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    // a room's draft is kept in this browser, past the test that typed it
    for (const room of Object.keys(roomDrafts)) delete roomDrafts[room];
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  let fetchCalls: () => { method: string; body: unknown }[] = () => [];
  const open = (target: Target = { kind: "room", name: "indie" }) => {
    const net = fakeNetwork(() => undefined);
    fetchCalls = () => net.requests;
    const pane = new MessagePane(target, () => "me");
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
    const press = (key: string, shiftKey = false) => {
      const passed = box.dispatchEvent(
        new KeyboardEvent("keydown", { key, shiftKey, bubbles: true, cancelable: true })
      );
      flushSync();
      return passed;
    };
    return { box, type, press, pane };
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

  it("completes an emoji or a name typed in full on Enter, sending nothing", () => {
    const { box, type, press } = open();
    const sent = () => fetchCalls().filter((r) => r.method === "POST");
    type(":smile");
    press("Enter");
    expect(box.value).toBe("😄 ");
    type("@alice");
    press("Enter");
    expect(box.value).toBe("@alice ");
    expect(sent()).toEqual([]);
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

  it("keeps a private message on one line and a room's as written", async () => {
    const note = "Line breaks become spaces in private messages";
    const room = open();
    expect(room.press("Enter", true)).toBe(true);
    room.type("one\ntwo");
    expect(document.body.textContent).not.toContain(note);
    unmount(component!);

    const chat = open({ kind: "user", name: "alice" });
    expect(chat.press("Enter", true)).toBe(false);
    expect(chat.box.value).toBe("");
    chat.type("one\ntwo\n");
    expect(document.body.textContent).toContain(note);
    chat.press("Enter");
    // the row shown at once is the text the daemon stores, which then takes its place
    expect(chat.pane.ordered.map((m) => m.text)).toEqual(["one two"]);
    expect(document.body.textContent).not.toContain(note);
  });

  it("quotes an action as what was done", async () => {
    const { box } = open();
    const quote = (component as unknown as { quote: (m: unknown) => void }).quote;
    quote({ author: "carol", text: "/me waves at everyone" });
    flushSync();
    expect(box.value).toBe("[carol] waves at everyone --> ");
  });
});
