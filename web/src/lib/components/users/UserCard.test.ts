import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";

import { box } from "#lib/testing/state.svelte.js";
import { firstLines } from "#lib/users/profile.svelte.js";

import UserCard from "./UserCard.svelte";

vi.mock("#lib/users/profile.svelte.js", async (load) => {
  const real = await load<typeof import("#lib/users/profile.svelte.js")>();
  return { ...real, firstLines: vi.fn(real.firstLines) };
});

afterEach(() => (document.body.innerHTML = ""));

it("cuts a draft's description once for each text, not on each change of the draft", () => {
  const draft: { description: string; src: string | null; pictureUrl: string | null } = {
    description: "one\ntwo\nthree\nfour",
    src: null,
    pictureUrl: null
  };
  const props = box({ name: "alice", user: undefined, onblock: () => {}, draft }).value;
  const card = mount(UserCard, { target: document.body, props });
  flushSync();
  expect(firstLines).toHaveBeenCalledOnce();
  expect(document.querySelector("p")?.textContent).toBe("one\ntwo\nthree");

  // a picture picked in the editor makes a new draft with the same text
  props.draft = { ...props.draft, pictureUrl: "blob:picture" };
  flushSync();
  expect(firstLines).toHaveBeenCalledOnce();

  props.draft = { ...props.draft, description: "five" };
  flushSync();
  expect(firstLines).toHaveBeenCalledTimes(2);
  expect(document.querySelector("p")?.textContent).toBe("five");
  unmount(card);
});
