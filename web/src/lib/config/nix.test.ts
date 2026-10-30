import { describe, expect, it } from "vitest";

import { asNixSettings } from "./nix";

describe("Copy as Nix", () => {
  it("writes strings so Nix reads back the same characters", () => {
    const text = 'a "q" \\ ${x} $${y}\n\t\b\f\u0001';
    expect(asNixSettings([["soulseek.description", text]])).toBe(
      `services.reseek.settings = {\n  soulseek = {\n    description = "a \\"q\\" \\\\ \\\${x} $\\\${y}\\n\\t\b\f\u0001";\n  };\n};\n`
    );
  });

  it("quotes a key that is not a Nix name", () => {
    expect(asNixSettings([["rooms", { "my room": true }]])).toBe(
      `services.reseek.settings = {\n  rooms = {\n    "my room" = true;\n  };\n};\n`
    );
  });
});
