import { afterEach, describe, expect, it, vi } from "vitest";

import { app, type Options } from "#lib/app.svelte.js";
import { changed } from "#lib/testing/fixtures.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { presetsOf, savePresets } from "./presets";

const big = { name: "Big", filter: "size:>100MB" };
const flac = { name: "FLAC", filter: "type:flac" };

/** The options with `presets` saved */
const holding = (...presets: unknown[]) =>
  ({ remote_configuration: true, search: { filter_presets: presets } }) as unknown as Options;

describe("saving the presets", () => {
  afterEach(() => {
    app.options = null;
    app.optionsRevision = null;
    vi.unstubAllGlobals();
  });

  it("keeps a frame that came before its answer", async () => {
    app.options = holding();
    app.optionsRevision = "first";
    fakeNetwork(() => {
      // the frame of this write, then of another admin's, reach the tab ahead of the answer
      app.options = holding(big, flac);
      app.optionsRevision = "third";
      return changed("second");
    });
    const outcome = await savePresets([big]);
    expect(outcome.ok).toBe(true);
    expect(app.optionsRevision).toBe("third");
    expect(presetsOf(app.options)).toEqual([big, flac]);
  });

  it("reads the options again once refused, so the next save goes through", async () => {
    app.options = holding();
    app.optionsRevision = "old";
    const net = fakeNetwork((url, method, body) => {
      if (method === "GET") return { values: holding(flac), revision: "now" };
      if ((body as { revision: string }).revision !== "now") {
        return {
          status: 409,
          body: { error: "The options changed since they were read", detail: "now" }
        };
      }
      return changed("after");
    });
    const refused = await savePresets([big]);
    expect(refused).toMatchObject({
      ok: false,
      error: "Filter presets changed meanwhile. Check your changes and save again."
    });
    expect(presetsOf(app.options)).toEqual([flac]);
    const saved = await savePresets([...presetsOf(app.options), big]);
    expect(saved.ok).toBe(true);
    expect(app.optionsRevision).toBe("after");
    expect(presetsOf(app.options)).toEqual([flac, big]);
    expect(net.requests.filter((r) => r.method === "PATCH")).toHaveLength(2);
  });
});
