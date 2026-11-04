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

  it("leaves out every secret the daemon hides, in a table's rows too", () => {
    const text = asNixSettings([
      ["web.authentication.password", "*****"],
      ["web.authentication.api_keys", { robot: { key: "*****", role: "administrator" } }],
      ["web.authentication.oidc", { home: { client_secret: "*****", issuer: "https://id.test" } }]
    ]);
    expect(text).not.toContain("*****");
    expect(text).not.toContain("password");
    expect(text).toContain('role = "administrator";');
    expect(text).toContain('issuer = "https://id.test";');
  });
});
