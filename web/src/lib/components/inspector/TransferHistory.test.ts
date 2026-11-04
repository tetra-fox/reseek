import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SETTLE_MS } from "#lib/inspector/inspector.svelte.js";
import { transfer } from "#lib/testing/fixtures.js";
import { flushFaked } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { box } from "#lib/testing/state.svelte.js";

import TransferHistory from "./TransferHistory.svelte";

describe("a transfer's history", () => {
  beforeEach(() => void vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("never shows the history of the transfer shown before", async () => {
    const a = transfer();
    const b = transfer();
    let answerA!: () => void;
    fakeNetwork((url) => {
      if (url.pathname.endsWith(a.id)) {
        return new Promise(
          (resolve) =>
            (answerA = () =>
              resolve({ ...a, stateHistory: [{ at: "2026-10-26T10:00:00Z", state: "Requested" }] }))
        );
      }
      if (url.pathname.endsWith(b.id)) {
        return { status: 500, body: { error: "boom" } };
      }
    });
    const shown = box(a);
    const component = mount(TransferHistory, {
      target: document.body,
      props: {
        get transfer() {
          return shown.value;
        }
      }
    });
    await vi.advanceTimersByTimeAsync(SETTLE_MS);
    await flushFaked();

    shown.value = b;
    flushSync();
    answerA();
    await flushFaked();
    expect(document.body.textContent).toContain("Loading");

    await vi.advanceTimersByTimeAsync(SETTLE_MS);
    await flushFaked();
    expect(document.body.textContent).toContain("Could not read the history: boom");
    unmount(component);
  });
});
