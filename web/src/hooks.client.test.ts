import { describe, expect, it } from "vitest";

import { handleError } from "./hooks.client";

const event = {} as never;

describe("a screen's error", () => {
  it("keeps its own message, where SvelteKit would say Internal Error", () => {
    expect(
      handleError({ kind: "unknown", error: new RangeError("Invalid time value"), event })
    ).toEqual({
      message: "Invalid time value"
    });
    expect(handleError({ kind: "unknown", error: "gone", event })).toEqual({ message: "gone" });
  });

  it("leaves SvelteKit's own errors as they are", () => {
    expect(
      handleError({ kind: "framework", error: { status: 404, message: "Not Found" }, event })
    ).toEqual({});
  });
});
