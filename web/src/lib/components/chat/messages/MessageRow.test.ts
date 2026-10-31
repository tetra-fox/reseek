import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";

import type { Message } from "#lib/chat/messages/messages.js";

import MessageRow from "./MessageRow.svelte";

const message = (fields: Partial<Message>): Message => ({
  localId: 1,
  timestamp: "2026-10-27T18:00:00Z",
  author: "alice",
  self: false,
  text: "hello from me",
  mention: false,
  hidden: false,
  replayed: false,
  unread: false,
  ...fields
});

describe("a message", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    document.body.innerHTML = "";
  });

  const render = (m: Message) => {
    component = mount(MessageRow, {
      target: document.body,
      props: {
        message: m,
        head: true,
        highlight: false,
        watch: ["me"],
        room: true,
        active: false,
        menu: false,
        onmenu: () => {},
        onretry: () => {}
      }
    });
    flushSync();
    return [...document.querySelectorAll("mark")].map((e) => e.textContent);
  };

  it("marks our name in another's message and not in our own", () => {
    expect(render(message({}))).toEqual(["me"]);
    unmount(component!);
    component = null;
    expect(render(message({ self: true, author: "me" }))).toEqual([]);
  });
});
