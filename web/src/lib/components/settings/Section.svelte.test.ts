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
    v.fields["web.port"].source = { layer: "file", file: "config.toml" };
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
    expect(web.querySelector<HTMLInputElement>('[id="web.port"]')!.value).toBe("5030");
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

  it("shows a saved secret as set again, with nothing left to save", async () => {
    const fetcher = vi.fn<(request: Request) => Promise<Response>>(
      async () =>
        new Response(
          JSON.stringify({
            revision: "sha256:2",
            applied: [],
            reconnectRequired: [],
            restartRequired: []
          }),
          { headers: { "Content-Type": "application/json" } }
        )
    );
    vi.stubGlobal("fetch", fetcher);
    const auth = render(sections.find((s) => s.id === "authentication")!);
    const status = () => auth.querySelector('[aria-live="polite"]')!.textContent!.trim();
    const key = "web.authentication.password";
    auth.querySelector<HTMLButtonElement>(`[id="${key}"]`)!.click();
    flushSync();
    const input = auth.querySelector<HTMLInputElement>(`input[id="${key}"]`)!;
    input.value = "new password";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    const save = [...auth.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Save"
    )!;
    save.click();
    await vi.waitFor(() => expect(status()).toBe("No changes"));
    const body = await fetcher.mock.calls[0][0].json();
    expect(body.set).toEqual({ [key]: "new password" });
    flushSync();
    expect(auth.querySelector(`input[id="${key}"]`)).toBeNull();
    expect(auth.querySelector(`button[id="${key}"]`)!.textContent).toContain("Change");
    expect(save.disabled).toBe(true);
    vi.unstubAllGlobals();
  });

  it("puts a changed option back to its default by taking it out of the file", async () => {
    const fetcher = vi.fn<(request: Request) => Promise<Response>>(
      async () =>
        new Response(
          JSON.stringify({
            revision: "sha256:2",
            applied: [],
            reconnectRequired: [],
            restartRequired: []
          }),
          { headers: { "Content-Type": "application/json" } }
        )
    );
    vi.stubGlobal("fetch", fetcher);
    const v = structuredClone(view);
    (v.values as { web: { port: number } }).web.port = 6000;
    v.fields["web.port"].source = { layer: "file", file: "config.toml" };
    const web = render(
      sections.find((s) => s.id === "web")!,
      v
    );
    const row = web.querySelector('[id="web.port-help"]')!.parentElement!;
    const reset = [...row.querySelectorAll("button")].find((b) =>
      b.title.startsWith("Changed from the default")
    )!;
    // a field already at its default has no marker
    expect(web.querySelector('[id="web.ui-help"]')!.parentElement!.textContent).not.toContain(
      "default\n"
    );
    reset.click();
    flushSync();
    expect(web.querySelector<HTMLInputElement>('[id="web.port"]')!.value).toBe("5030");
    [...web.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Save")!.click();
    await vi.waitFor(() => expect(fetcher).toHaveBeenCalled());
    const body = await fetcher.mock.calls[0][0].json();
    expect(body).toMatchObject({ set: {}, unset: ["web.port"] });
    vi.unstubAllGlobals();
  });

  it("shows only the options the search finds, keeping the others' edits", () => {
    const target = document.createElement("div");
    document.body.append(target);
    const props = $state({
      section: sections.find((s) => s.id === "web")!,
      view,
      onchanged: async () => {},
      query: ""
    });
    mounted.push(mount(Section, { target, props }));
    flushSync();
    const port = () => target.querySelector<HTMLInputElement>('[id="web.port"]')!;
    port().value = "6001";
    port().dispatchEvent(new Event("input", { bubbles: true }));
    props.query = "url base";
    flushSync();
    const shown = [...target.querySelectorAll("label")].filter((l) => !l.closest(".hidden"));
    const keys = shown.map((l) => l.getAttribute("for"));
    expect(keys).toContain("web.url_base");
    expect(keys).not.toContain("web.port");
    props.query = "";
    flushSync();
    expect(port().value).toBe("6001");
  });
});
