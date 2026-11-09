import { describe, expect, it } from "vitest";

import { pageCount, pagerItems, placeIn, withPlace } from "./pages";

describe("paged lists", () => {
  it("shows the first, the last and the pages around the current one", () => {
    expect(pagerItems(1, 1)).toEqual([1]);
    expect(pagerItems(1, 5)).toEqual([1, 2, "gap", 5]);
    expect(pagerItems(5, 10)).toEqual([1, "gap", 4, 5, 6, "gap", 10]);
    // a gap of one page shows the page
    expect(pagerItems(4, 10)).toEqual([1, 2, 3, 4, 5, "gap", 10]);
    expect(pagerItems(10, 10)).toEqual([1, "gap", 9, 10]);
  });

  it("reads and writes the place in the address, leaving the defaults out", () => {
    const sizes = { sizes: [25, 50, 100], size: 50 };
    const at = (text: string) => placeIn(new URLSearchParams(text), sizes);
    expect(at("page=3&per_page=100")).toEqual({ page: 3, size: 100 });
    expect(at("page=-2&per_page=7")).toEqual({ page: 1, size: 50 });
    expect(at("page=x")).toEqual({ page: 1, size: 50 });
    const query = new URLSearchParams("group=jazz&page=4");
    expect(withPlace(query, { page: 1, size: 50 }, sizes).toString()).toBe("group=jazz");
    expect(withPlace(query, { page: 2, size: 25 }, sizes).toString()).toBe(
      "group=jazz&page=2&per_page=25"
    );
    expect(pageCount(0, 50)).toBe(1);
    expect(pageCount(101, 50)).toBe(3);
  });
});
