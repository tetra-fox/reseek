import { describe, expect, it } from "vitest";

import { editability } from "./editability";
import { type ConfigView, controlOf, errorsByKey, fromDraft, leftovers, toDraft } from "./fields";
import fixture from "./fixtures/view.json";
import { asNixSettings } from "./nix";
import { sections, valueAt } from "./sections";

/**
 * The daemon's options view of the default configuration with an empty config.toml, written by
 * `just config-fixture`; `just api-check` fails once the daemon's options differ from it
 */
const view = fixture as unknown as ConfigView;
const copy = (): ConfigView => structuredClone(view);

describe("the settings forms", () => {
  it("cover every section of the configuration, in its order", () => {
    expect(sections.map((s) => s.title)).toEqual([
      "Soulseek",
      "Shares",
      "Transfers",
      "Groups",
      "Folders",
      "Web",
      "Authentication",
      "Search filters",
      "Retention",
      "Throttling",
      "Integrations",
      "Logging",
      "Metrics",
      "Relay",
      "Flags"
    ]);
  });

  it("name only options the daemon has, each once", () => {
    const keys = sections.flatMap((s) => s.fields.map((f) => f.key));
    expect(keys.filter((key) => !(key in view.fields))).toEqual([]);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("leave the rest to Everything else", () => {
    const rest = leftovers(view).fields.map((f) => f.key);
    const named = sections.flatMap((s) => s.fields.map((f) => f.key));
    expect([...named, ...rest].sort()).toEqual(Object.keys(view.fields).sort());
    expect(rest).toContain("chat.max_message_length");
  });
});

describe("controls and drafts", () => {
  const field = (key: string) => sections.flatMap((s) => s.fields).find((f) => f.key === key)!;

  it("follow the daemon's kind unless the description says otherwise", () => {
    expect(controlOf(field("web.port"), view.fields["web.port"])).toBe("number");
    expect(controlOf(field("web.ui"), view.fields["web.ui"])).toBe("bool");
    expect(controlOf(field("rooms"), view.fields["rooms"])).toBe("list");
    expect(controlOf(field("relay.mode"), view.fields["relay.mode"])).toBe("select");
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
    expect(fromDraft(field("relay.agents"), "map", '{"a":{"secret":"*****"}}')).toEqual({
      set: { a: { secret: "*****" } }
    });
    expect(fromDraft(field("relay.agents"), "map", "[]")).toMatchObject({
      error: expect.any(String)
    });
    expect(fromDraft(field("soulseek.password"), "secret", "")).toEqual({ set: "" });
    expect(fromDraft(field("relay.mode"), "select", "sideways", view.fields["relay.mode"])).toEqual(
      { error: "must be one of controller, agent, debug" }
    );
    expect(fromDraft(field("relay.mode"), "select", "agent", view.fields["relay.mode"])).toEqual({
      set: "agent"
    });
  });

  it("show running values in the control", () => {
    expect(toDraft("list", ["a", "b"])).toBe("a\nb");
    expect(toDraft("number", null)).toBe("");
    expect(toDraft("bool", true)).toBe(true);
    expect(toDraft("map", { a: 1 })).toBe('{\n  "a": 1\n}');
  });

  it("place the daemon's refusals at their fields", () => {
    const found = errorsByKey("web.port: must be at least 1; web.ui: no", ["web.port", "web.ui"]);
    expect(Object.fromEntries(found)).toEqual({ "web.port": "must be at least 1", "web.ui": "no" });
  });
});

describe("editability", () => {
  it("saves to a writable file, and says when a restart is needed", () => {
    expect(editability(view, "soulseek.description")).toEqual({ mode: "save", notice: null });
    expect(editability(view, "web.port")).toEqual({
      mode: "save",
      notice: "Needs a restart to take effect"
    });
  });

  it("locks what the environment or the command line set", () => {
    const v = copy();
    v.fields["soulseek.username"].source = { layer: "env", var: "RESEEK_SLSK_USERNAME" };
    v.fields["web.port"].source = { layer: "cli", flag: "--http-port" };
    expect(editability(v, "soulseek.username")).toEqual({
      mode: "locked",
      notice: "Set by RESEEK_SLSK_USERNAME. Change it there."
    });
    expect(editability(v, "web.port").notice).toBe(
      "Set on the command line with --http-port. Change it there."
    );
  });

  it("applies at runtime what a managed file holds, unless it needs a restart", () => {
    const v = copy();
    v.managedBy = "nixos";
    v.files[0].writable = false;
    expect(editability(v, "soulseek.description").mode).toBe("runtime");
    expect(editability(v, "soulseek.description").notice).toContain("Copy as Nix");
    expect(editability(v, "web.port")).toMatchObject({ mode: "locked" });
  });

  it("names why a file can't be written", () => {
    const v = copy();
    v.files[0].writable = false;
    v.files[0].readOnlyReason = "permission denied";
    expect(editability(v, "soulseek.description")).toEqual({
      mode: "runtime",
      notice: "config.toml isn't writable (permission denied). Changes apply now but won't persist."
    });
  });
});

describe("Copy as Nix", () => {
  it("nests dotted keys and escapes interpolation", () => {
    expect(
      asNixSettings([
        ["web.port", 5030],
        ["shares.directories", []],
        ["a.b", "${x}"]
      ])
    ).toBe(
      'services.reseek.settings = {\n  web = {\n    port = 5030;\n  };\n  shares = {\n    directories = [ ];\n  };\n  a = {\n    b = "\\${x}";\n  };\n};\n'
    );
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
    expect(fromDraft(presets, "table-list", '["type:flac"]', info)).toEqual({
      error: "must be a JSON list of objects"
    });
  });
});
