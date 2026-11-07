import { describe, expect, it } from "vitest";

import { editability } from "./editability";
import type { ConfigView } from "./fields";
import fixture from "./fixtures/view.json";

/** the daemon's options view of the default configuration, written by `just config-fixture` */
const view = fixture as unknown as ConfigView;

describe("editability", () => {
  const copy = (): ConfigView => structuredClone(view);

  it("saves to a writable file", () => {
    expect(editability(copy(), "soulseek.description")).toEqual({ mode: "save", notice: null });
    expect(editability(copy(), "web.port")).toEqual({ mode: "save", notice: null });
  });

  it("locks everything while remote configuration is off, which the page says once", () => {
    const v = copy();
    (v.values as Record<string, unknown>).remote_configuration = false;
    expect(editability(v, "soulseek.description")).toEqual({ mode: "locked", notice: null });
  });

  it("locks what the environment or the command line set, naming where", () => {
    const v = copy();
    v.fields["soulseek.username"].source = { layer: "env", var: "RESEEK_SLSK_USERNAME" };
    v.fields["web.port"].source = { layer: "cli", flag: "--http-port" };
    expect(editability(v, "soulseek.username")).toEqual({
      mode: "locked",
      notice: "Set by RESEEK_SLSK_USERNAME"
    });
    expect(editability(v, "web.port").notice).toBe("Set by --http-port");
  });

  it("applies at runtime what a managed file holds, unless it needs a restart", () => {
    const v = copy();
    v.managedBy = "Nix";
    v.files[0].writable = false;
    expect(editability(v, "soulseek.description")).toEqual({ mode: "runtime", notice: null });
    expect(editability(v, "web.port")).toEqual({
      mode: "locked",
      notice: "Change it in the Nix configuration"
    });
  });

  it("says on a page that keeps a change until removed what it overrides", () => {
    const v = copy();
    v.files[0].writable = false;
    v.files[0].readOnlyReason = "permission denied";
    expect(editability(v, "soulseek.description", "page")).toEqual({
      mode: "runtime",
      notice:
        "config.toml is read-only (permission denied), so a change here overrides it until removed"
    });
    v.managedBy = "Nix";
    expect(editability(v, "soulseek.description", "page").notice).toBe(
      "Overrides the Nix configuration until removed"
    );
    expect(editability(v, "web.port", "page")).toEqual(editability(v, "web.port"));
  });

  it("names why a file can't be written", () => {
    const v = copy();
    v.files[0].writable = false;
    v.files[0].readOnlyReason = "permission denied";
    expect(editability(v, "soulseek.description")).toEqual({
      mode: "runtime",
      notice: "config.toml is read-only (permission denied), so changes here are temporary"
    });
  });

  it("says whether a value applied at runtime lasts until restart or until removed", () => {
    const v = copy();
    v.files[0].writable = false;
    const applied = (untilRestart: boolean) => {
      v.fields["soulseek.description"].source = {
        layer: "runtime",
        setBy: "admin",
        setAt: "2026-10-29T10:00:00Z",
        expiresAt: null,
        untilRestart
      };
      return editability(v, "soulseek.description").notice;
    };
    expect(applied(true)).toBe("Applied here until restart");
    expect(applied(false)).toBe("Applied here until removed");
  });
});
