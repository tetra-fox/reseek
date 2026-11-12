import { describe, expect, it } from "vitest";

import { ORDER, USER_DEFINED, VISIBLE_TO } from "#lib/users/groups.js";

import { type ConfigView, controlOf, errorsByKey, fromDraft, toDraft } from "./fields";
import fixture from "./fixtures/view.json";
import { BLOCKING, sections, valueAt } from "./sections";

/**
 * Options Settings does not show: `relay.mode` takes only `controller` and is kept so that a
 * configuration still naming the daemon's old roles fails with a pointer to `reseek-agent`;
 * whether the daemon is a controller is `relay.enabled`
 */
const unlisted = new Set(["relay.mode"]);

/**
 * The daemon's options view of the default configuration with an empty config.toml, written by
 * `just config-fixture`; `just lint` fails once the daemon's options differ from it
 */
const view = fixture as unknown as ConfigView;

describe("the settings forms", () => {
  it("stand together under each heading of the side list", () => {
    const groups = sections.map((s) => s.group);
    expect(groups.filter((g, i) => g !== groups[i - 1])).toEqual(["Network", "Storage", "Server"]);
  });

  it("name only options the daemon has, each once", () => {
    const keys = [...sections, BLOCKING].flatMap((s) => s.fields.map((f) => f.key));
    expect(keys.filter((key) => !(key in view.fields))).toEqual([]);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("give every option of the daemon a section and a name of its own", () => {
    const fields = [...sections, BLOCKING].flatMap((s) => s.fields);
    const named = new Set(fields.map((f) => f.key));
    // the groups page edits the groups, their order, their settings and who sees each share
    const onGroupsPage = (key: string) =>
      [ORDER, USER_DEFINED, VISIBLE_TO].includes(key) ||
      /^transfers\.groups\.(default|buddies|leechers)\.upload\./.test(key) ||
      key.startsWith("transfers.groups.leechers.thresholds.");
    expect(
      Object.keys(view.fields).filter(
        (key) => !named.has(key) && !unlisted.has(key) && !onGroupsPage(key)
      )
    ).toEqual([]);
    expect(fields.filter((f) => f.label === f.key)).toEqual([]);
  });

  it("leave out only options with a single choice, which there is nothing to set", () => {
    const named = new Set(sections.flatMap((s) => s.fields.map((f) => f.key)));
    for (const key of unlisted) {
      expect([key, view.fields[key].choices?.length]).toEqual([key, 1]);
      expect([key, named.has(key)]).toEqual([key, false]);
    }
  });

  it("label every field of every table", () => {
    const fields = sections.flatMap((s) => s.fields);
    for (const field of fields) {
      const unlabeled = view.fields[field.key].entries
        .map((entry) => entry.key)
        .filter((key) => !field.columns?.[key]);
      expect([field.key, unlabeled]).toEqual([field.key, []]);
    }
  });

  it("leave no option a raw JSON box", () => {
    for (const field of sections.flatMap((s) => s.fields)) {
      const control = controlOf(field, view.fields[field.key]);
      if (control !== "map" && control !== "table-list") continue;
      expect(view.fields[field.key].entries.length).toBeGreaterThan(0);
    }
  });
});

describe("controls and drafts", () => {
  const field = (key: string) => sections.flatMap((s) => s.fields).find((f) => f.key === key)!;

  it("follow the daemon's kind unless the description says otherwise", () => {
    expect(controlOf(field("web.port"), view.fields["web.port"])).toBe("number");
    expect(controlOf(field("web.ui"), view.fields["web.ui"])).toBe("bool");
    expect(controlOf(field("rooms"), view.fields["rooms"])).toBe("list");
    expect(
      controlOf(field("shares.cache.storage_mode"), view.fields["shares.cache.storage_mode"])
    ).toBe("select");
    for (const key of ["chat.notifications.mentions", "notifications.transfers.job_finished"]) {
      expect(controlOf(field(key), view.fields[key])).toBe("notify-mode");
    }
    expect(controlOf(field("soulseek.password"), view.fields["soulseek.password"])).toBe("secret");
    expect(
      controlOf({ key: "x", label: "x" }, view.fields["metrics.authentication.password"])
    ).toBe("secret");
    expect(controlOf(field("integrations.scripts"), view.fields["integrations.scripts"])).toBe(
      "map"
    );
    expect(controlOf(field("retention.events"), view.fields["retention.events"])).toBe("duration");
  });

  it("parse what was typed, and take an emptied input as unset", () => {
    const port = field("web.port");
    expect(fromDraft(port, "number", "8080", view.fields["web.port"])).toEqual({ set: 8080 });
    expect(fromDraft(port, "number", "", view.fields["web.port"])).toEqual({ unset: true });
    expect(fromDraft(port, "number", "0", view.fields["web.port"])).toEqual({
      error: "must be at least 1"
    });
    expect(fromDraft(port, "number", "1.5", view.fields["web.port"])).toEqual({
      error: "must be a whole number"
    });
    expect(fromDraft(port, "number", "x", view.fields["web.port"])).toEqual({
      error: "must be a number"
    });
    const workers = field("analysis.workers");
    expect(fromDraft(workers, "auto-count", " Auto ")).toEqual({ set: "auto" });
    expect(fromDraft(workers, "auto-count", "3")).toEqual({ set: 3 });
    expect(fromDraft(workers, "auto-count", "0")).toEqual({ error: "must be at least 1" });
    expect(fromDraft(workers, "auto-count", "many")).toEqual({
      error: "must be auto or a number"
    });
    // scan workers hold auto as no value
    const scan = field("shares.cache.workers");
    expect(toDraft("auto-count", null)).toBe("auto");
    expect(fromDraft(scan, "auto-count", "auto")).toEqual({ unset: true });
    expect(fromDraft(scan, "auto-count", "4")).toEqual({ set: 4 });
    expect(fromDraft(field("rooms"), "list", " a \n\n b ")).toEqual({ set: ["a", "b"] });
    expect(fromDraft(field("shares.filters"), "regex-list", "(")).toMatchObject({
      error: expect.any(String)
    });
    expect(fromDraft(field("web.trusted_proxies"), "cidr-list", "10.0.0.0/8\n::1")).toEqual({
      set: ["10.0.0.0/8", "::1"]
    });
    expect(fromDraft(field("web.trusted_proxies"), "cidr-list", "lan")).toMatchObject({
      error: expect.any(String)
    });
    expect(fromDraft(field("relay.agents"), "map", '[["a",{"secret":"*****"}]]')).toEqual({
      set: { a: { secret: "*****" } }
    });
    expect(fromDraft(field("relay.agents"), "map", "[]")).toEqual({ set: {} });
    expect(fromDraft(field("soulseek.password"), "secret", "")).toEqual({ set: "" });
    const storage = view.fields["shares.cache.storage_mode"];
    expect(fromDraft(field("shares.cache.storage_mode"), "select", "sideways", storage)).toEqual({
      error: "must be one of memory, disk"
    });
    expect(fromDraft(field("shares.cache.storage_mode"), "select", "disk", storage)).toEqual({
      set: "disk"
    });
  });

  it("show running values in the control", () => {
    expect(toDraft("list", ["a", "b"])).toBe("a\nb");
    expect(toDraft("number", null)).toBe("");
    expect(toDraft("bool", true)).toBe(true);
    expect(toDraft("map", { a: { b: null, c: 1 } })).toBe('[["a",{"c":1}]]');
  });

  it("place the daemon's refusals at their fields", () => {
    const found = errorsByKey("web.port: must be at least 1; web.ui: no", ["web.port", "web.ui"]);
    expect(Object.fromEntries(found)).toEqual({ "web.port": "must be at least 1", "web.ui": "no" });
    expect(errorsByKey("a: one; a: two", ["a"]).get("a")).toBe("one; two");
    // a value its type refuses, as the daemon words it; a table's names the row's field
    const typed = errorsByKey(
      'analysis.timeout: invalid value: string "five", expected a duration; ' +
        "transfers.groups: invalid type: floating point `1.5`, expected u32 " +
        "(at transfers.groups.friends.upload.slots)",
      ["analysis.timeout", "transfers.groups"]
    );
    expect(Object.fromEntries(typed)).toEqual({
      "analysis.timeout": 'invalid value: string "five", expected a duration',
      "transfers.groups":
        "invalid type: floating point `1.5`, expected u32 (at transfers.groups.friends.upload.slots)"
    });
  });
});

describe("an option made of tables", () => {
  it("is edited as JSON, so no option's value shows as [object Object]", () => {
    for (const [key, info] of Object.entries(view.fields)) {
      const field = { key, label: key };
      const value = valueAt(view.values, key);
      const draft = toDraft(controlOf(field, info), value);
      expect([key, String(draft)]).not.toEqual([key, expect.stringContaining("[object Object]")]);
    }
    const presets = { key: "search.filter_presets", label: "Filter presets" };
    const info = view.fields["search.filter_presets"];
    expect(controlOf(presets, info)).toBe("table-list");
    expect(
      fromDraft(presets, "table-list", '[{"name": "FLAC", "filter": "type:flac"}]', info)
    ).toEqual({
      set: [{ name: "FLAC", filter: "type:flac" }]
    });
    expect(fromDraft(presets, "table-list", "[{}]", info)).toEqual({ error: "This row is empty" });
  });
});
