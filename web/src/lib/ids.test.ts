import { describe, expect, it } from "vitest";

import { newId } from "./ids";

describe("a new id", () => {
  it("is a version 4 UUID of the RFC 4122 variant, different each time", () => {
    const ids = Array.from({ length: 200 }, newId);
    for (const id of ids) {
      expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    }
    expect(new Set(ids).size).toBe(ids.length);
  });
});
