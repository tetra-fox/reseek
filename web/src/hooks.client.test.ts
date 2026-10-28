import { describe, expect, it, vi } from "vitest";

import { handleError } from "./hooks.client";

const event = {} as never;

describe("a screen's error", () => {
  it("keeps its own message, where SvelteKit would say Internal Error, and logs it", () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    const thrown = new RangeError("Invalid time value");
    expect(handleError({ kind: "unknown", error: thrown, event })).toEqual({
      message: "Invalid time value"
    });
    expect(logged).toHaveBeenCalledWith(thrown);
    expect(handleError({ kind: "unknown", error: "gone", event })).toEqual({ message: "gone" });
    logged.mockRestore();
  });

  it("leaves SvelteKit's own errors as they are", () => {
    expect(
      handleError({ kind: "framework", error: { status: 404, message: "Not Found" }, event })
    ).toEqual({});
  });
});
