import { describe, expect, it } from "vitest";

import { focusAfter, hiding, showing } from "./filtered";

const DOWNLOADS = ["download", "downloads"] as const;

describe("a filtered list's summary", () => {
  it("counts what the filters leave out of everything", () => {
    expect(showing(12, 340, DOWNLOADS)).toBe("Showing 12 of 340 downloads");
    expect(showing(0, 1, DOWNLOADS)).toBe("Showing 0 of 1 download");
    expect(showing(1200, 3400, DOWNLOADS)).toBe("Showing 1,200 of 3,400 downloads");
    // filters on that leave everything
    expect(showing(5, 5, DOWNLOADS)).toBe("Showing all 5 downloads");
    expect(showing(1, 1, DOWNLOADS)).toBe("Showing the only download");
  });

  it("counts only the matches of a list that does not know its whole", () => {
    expect(showing(12, null, ["user", "users"])).toBe("12 users match");
    expect(showing(1, null, ["user", "users"])).toBe("1 user matches");
  });

  it("says why an emptied list is empty", () => {
    expect(hiding(340, DOWNLOADS)).toBe("The filters hide all 340 downloads");
    expect(hiding(1, DOWNLOADS)).toBe("The filters hide the only download");
    expect(hiding(null, ["user", "users"])).toBe("No users match the filters");
  });

  it("moves focus to the chip after the removed one, else the one before", () => {
    expect(focusAfter(0, 3)).toBe(1);
    expect(focusAfter(1, 3)).toBe(2);
    // nothing comes after the last one
    expect(focusAfter(2, 3)).toBe(1);
    // the only one leaves no chip
    expect(focusAfter(0, 1)).toBeNull();
  });
});
