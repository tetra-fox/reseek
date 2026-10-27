import { describe, expect, it } from "vitest";

import { type Arrival, type NotifyOptions, shouldNotify } from "./notify.svelte";

const options: NotifyOptions = {
  enabled: true,
  onlyWhenHidden: true,
  sound: false,
  privateMessages: true,
  mentions: true,
  roomMessages: false
};
const arrival = (fields: Partial<Arrival>): Arrival => ({
  kind: "private",
  level: "all",
  mention: false,
  muted: false,
  seen: false,
  ...fields
});

describe("which messages notify", () => {
  it("notifies private messages and room mentions by default, plain room messages not", () => {
    expect(shouldNotify(arrival({}), options)).toBe(true);
    expect(shouldNotify(arrival({ kind: "room", level: "mentions", mention: true }), options)).toBe(
      true
    );
    expect(shouldNotify(arrival({ kind: "room", level: "all" }), options)).toBe(false);
    expect(
      shouldNotify(arrival({ kind: "room", level: "all" }), { ...options, roomMessages: true })
    ).toBe(true);
    expect(
      shouldNotify(arrival({ kind: "room", level: "mentions" }), { ...options, roomMessages: true })
    ).toBe(false);
  });

  it("stays quiet for a muted user, a level of none, a pane in view, or options off", () => {
    expect(shouldNotify(arrival({ muted: true }), options)).toBe(false);
    expect(shouldNotify(arrival({ level: "none" }), options)).toBe(false);
    expect(shouldNotify(arrival({ seen: true }), options)).toBe(false);
    expect(shouldNotify(arrival({ seen: true }), { ...options, onlyWhenHidden: false })).toBe(true);
    expect(shouldNotify(arrival({}), { ...options, enabled: false })).toBe(false);
    expect(shouldNotify(arrival({}), { ...options, privateMessages: false })).toBe(false);
    expect(
      shouldNotify(arrival({ kind: "room", mention: true }), { ...options, mentions: false })
    ).toBe(false);
  });
});
