import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { type ConfigView, leftovers } from "#lib/config/fields.js";
import fixture from "#lib/config/fixtures/view.json";
import { sections } from "#lib/config/sections.js";

import Section from "./Section.svelte";

const view = fixture as unknown as ConfigView;

describe("a settings section", () => {
  let mounted: ReturnType<typeof mount>[] = [];
  afterEach(() => {
    for (const m of mounted) unmount(m);
    mounted = [];
    document.body.innerHTML = "";
  });

  const render = (section: (typeof sections)[number], v: ConfigView = view) => {
    const target = document.createElement("div");
    document.body.append(target);
    mounted.push(
      mount(Section, { target, props: { section, view: v, onchanged: async () => {} } })
    );
    flushSync();
    return target;
  };

  it("renders a row for every option of every section, with its help and control", () => {
    for (const section of [...sections, leftovers(view)]) {
      const target = render(section);
      const labels = [...target.querySelectorAll("label")].map((l) => l.getAttribute("for"));
      expect(labels).toEqual(section.fields.map((f) => f.key));
      for (const field of section.fields) {
        expect(target.querySelector(`[id="${field.key}-help"]`)!.textContent).toContain(
          view.fields[field.key].doc
        );
        expect(target.querySelector(`[id="${field.key}"]`)).not.toBeNull();
      }
    }
  });

  it("shows where each value comes from and when a change applies", () => {
    const v = structuredClone(view);
    v.fields["web.ui"].source = { layer: "env", var: "RESEEK_WEB_UI" };
    const web = render(
      sections.find((s) => s.id === "web")!,
      v
    );
    const row = (key: string) => web.querySelector(`[id="${key}-help"]`)!.parentElement!;
    expect(row("web.port").textContent).toContain("config.toml");
    expect(row("web.port").textContent).toContain("restart");
    expect(row("web.ui").textContent).toContain("RESEEK_WEB_UI");
    expect(row("web.external_url").textContent).not.toContain("config.toml");
    expect(web.querySelector<HTMLInputElement>('[id="web.port"]')!.value).toBe("5130");
    expect(web.querySelector<HTMLButtonElement>('[id="web.ui"]')!.disabled).toBe(true);
  });

  it("changes a secret only once something is typed, or it is cleared on purpose", () => {
    const auth = render(sections.find((s) => s.id === "authentication")!);
    const status = () => auth.querySelector('[aria-live="polite"]')!.textContent!.trim();
    const key = "web.authentication.password";
    auth.querySelector<HTMLButtonElement>(`[id="${key}"]`)!.click();
    flushSync();
    expect(status()).toBe("No changes");
    const input = auth.querySelector<HTMLInputElement>(`input[id="${key}"]`)!;
    const type = (text: string) => {
      input.value = text;
      input.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
    };
    type("new password");
    expect(status()).toBe("1 setting changed");
    type("");
    expect(status()).toBe("No changes");
    const clear = [...auth.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Clear"
    )!;
    clear.click();
    flushSync();
    expect(status()).toBe("1 setting changed");
  });

  it("tests each configured identity provider's discovery and shows what it found", async () => {
    const v = structuredClone(view);
    (v.values as { web: { authentication: { oidc: unknown } } }).web.authentication.oidc = {
      home: { issuer: "https://id.example" }
    };
    const answer = {
      ok: true,
      issuer: "https://id.example",
      error: null,
      endpoints: { authorization: "https://id.example/auth", token: "t", jwks: "j" }
    };
    const fetcher = vi.fn<(request: Request) => Promise<Response>>(
      async () =>
        new Response(JSON.stringify(answer), { headers: { "Content-Type": "application/json" } })
    );
    vi.stubGlobal("fetch", fetcher);
    const auth = render(
      sections.find((s) => s.id === "authentication")!,
      v
    );
    const button = [...auth.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Test discovery: home")
    )!;
    button.click();
    await vi.waitFor(() => expect(auth.textContent).toContain("passed"));
    expect(new URL(fetcher.mock.calls[0][0].url).pathname).toBe("/api/v0/auth/oidc/home/test");
    expect(auth.textContent).toContain("https://id.example/auth");
    vi.unstubAllGlobals();
  });
});
