import { afterEach, describe, expect, it, vi } from "vitest";

import { written } from "#lib/testing/fixtures.js";
import { fakeNetwork } from "#lib/testing/network.js";

import type { ConfigView } from "./fields";
import fixture from "./fixtures/view.json";
import { writeFromPage } from "./write";

const view = structuredClone(fixture) as unknown as ConfigView;

describe("a write from a page", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is refused once the options changed since the view, saying what changed", async () => {
    // another admin made a group after this page read the options
    const now = structuredClone(view);
    now.revision = "second";
    (
      now.values as { transfers: { groups: { user_defined: unknown } } }
    ).transfers.groups.user_defined = {
      friends: {}
    };
    const network = fakeNetwork((_url, method) => {
      if (method === "PATCH") {
        return {
          status: 409,
          body: { error: "The options changed since they were read", detail: "second" }
        };
      }
      return undefined;
    });
    const views = { data: view, load: async () => void (views.data = now) };
    const outcome = await writeFromPage(views, { "transfers.groups.user_defined": { mine: {} } });
    expect(outcome).toMatchObject({
      ok: false,
      status: 409,
      error: "Groups changed meanwhile. Check your changes and save again."
    });
    expect(network.requests[0].body).toMatchObject({ optionsRevision: "fixture" });
  });

  it("writes two files at the revision each write left, then reads the options again", async () => {
    const two = structuredClone(view);
    two.files.push({ ...two.files[0], name: "config.d/10-groups.toml" });
    two.fields["transfers.groups.order"].file = "config.d/10-groups.toml";
    const network = fakeNetwork((_url, method, body) =>
      method === "PATCH"
        ? written({ optionsRevision: `after ${(body as { file: string }).file}` })
        : undefined
    );
    let reads = 0;
    const views = { data: two, load: async () => void (reads += 1) };
    const outcome = await writeFromPage(views, {
      "transfers.groups.user_defined": { mine: {} },
      "transfers.groups.order": ["mine"]
    });
    expect(outcome.ok).toBe(true);
    const sent = network.requests.map((r) => r.body as { file: string; optionsRevision: string });
    expect(sent.map((b) => b.optionsRevision)).toEqual(["fixture", `after ${sent[0].file}`]);
    expect(reads).toBe(1);
  });

  it("writes the files in the order of their first key", async () => {
    const two = structuredClone(view);
    two.files.push({ ...two.files[0], name: "config.d/10-groups.toml" });
    two.fields["transfers.groups.order"].file = "config.d/10-groups.toml";
    const network = fakeNetwork((_url, method) =>
      method === "PATCH" ? written({ optionsRevision: "after" }) : undefined
    );
    const views = { data: two, load: async () => {} };
    await writeFromPage(views, {
      "transfers.groups.order": ["mine"],
      "transfers.groups.user_defined": { mine: {} }
    });
    const files = network.requests.map((r) => (r.body as { file: string }).file);
    expect(files).toEqual(["config.d/10-groups.toml", "config.toml"]);
  });
});
