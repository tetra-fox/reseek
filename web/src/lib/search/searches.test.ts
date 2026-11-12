import { describe, expect, it } from "vitest";

import { isDone, searchStage, type SearchState } from "./searches.svelte";

describe("search stages", () => {
  it("read a state this page does not know as unknown, neither running nor done", () => {
    const newer = "Paused" as SearchState;
    expect(searchStage(newer)).toBe("unknown");
    expect(isDone(newer)).toBe(false);
    expect(searchStage("toString" as SearchState)).toBe("unknown");
    expect(isDone("Completed, Cancelled")).toBe(true);
    expect(isDone("Queued")).toBe(false);
  });
});
