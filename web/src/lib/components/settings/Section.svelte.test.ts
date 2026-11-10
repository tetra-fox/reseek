import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ConfigView } from "#lib/config/fields.js";
import fixture from "#lib/config/fixtures/view.json";
import { sections } from "#lib/config/sections.js";
import { toasts } from "#lib/feedback/toast.svelte.js";
import { written } from "#lib/testing/fixtures.js";
import { flush } from "#lib/testing/flush.js";

import Section from "./Section.svelte";

const view = structuredClone(fixture) as unknown as ConfigView;

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

  it.each(sections.map((section) => [section.id, section] as const))(
    "has a control for every option of the %s section",
    (_, section) => {
      render(section);
      for (const field of section.fields) {
        expect(document.getElementById(field.key), field.key).not.toBeNull();
      }
    }
  );

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
    await vi.waitFor(() => expect(auth.textContent).toContain("Passed"));
    expect(new URL(fetcher.mock.calls[0][0].url).pathname).toBe("/api/v0/auth/oidc/home/test");
    expect(auth.textContent).toContain("https://id.example/auth");
    vi.unstubAllGlobals();
  });

  it("shows a saved secret as set again, with nothing left to save", async () => {
    const fetcher = vi.fn<(request: Request) => Promise<Response>>(async () =>
      Response.json(written())
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

  it("keeps a table's hidden secret unless something is typed in its place", async () => {
    const fetcher = vi.fn<(request: Request) => Promise<Response>>(async () =>
      Response.json(written())
    );
    vi.stubGlobal("fetch", fetcher);
    const v = structuredClone(view);
    (v.values as { web: { authentication: { api_keys: unknown } } }).web.authentication.api_keys = {
      robot: { key: "*****", role: "administrator", cidr: "10.0.0.0/8" }
    };
    const auth = render(
      sections.find((s) => s.id === "authentication")!,
      v
    );
    const status = () => auth.querySelector('[aria-live="polite"]')!.textContent!.trim();
    const type = (label: string, text: string) => {
      const input = auth.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`)!;
      input.value = text;
      input.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
    };
    /** Saves, and returns the robot entry the save sent */
    const saved = async () => {
      const calls = fetcher.mock.calls.length;
      [...auth.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Save")!.click();
      await vi.waitFor(() => expect(status()).toBe("No changes"));
      const body = await fetcher.mock.calls[calls][0].json();
      return body.set["web.authentication.api_keys"].robot;
    };

    // another cell changed: the secret goes back as it was shown
    type("Networks of robot", "192.168.0.0/16");
    expect(await saved()).toEqual({ key: "*****", role: "administrator", cidr: "192.168.0.0/16" });
    // typed into and emptied again: still kept, not taken out
    type("Networks of robot", "10.0.0.1/32");
    type("Key of robot", "a");
    type("Key of robot", "");
    expect(await saved()).toEqual({ key: "*****", role: "administrator", cidr: "10.0.0.1/32" });
    // typed: sent
    type("Key of robot", "robot-key-0123456789");
    expect(await saved()).toEqual({
      key: "robot-key-0123456789",
      role: "administrator",
      cidr: "10.0.0.0/8"
    });
    vi.unstubAllGlobals();
  });

  it("puts a changed option back to its default by taking it out of the file", async () => {
    const fetcher = vi.fn<(request: Request) => Promise<Response>>(async () =>
      Response.json(written())
    );
    vi.stubGlobal("fetch", fetcher);
    const v = structuredClone(view);
    (v.values as { web: { port: number } }).web.port = 6000;
    v.fields["web.port"].source = { layer: "file", file: "config.toml" };
    const web = render(
      sections.find((s) => s.id === "web")!,
      v
    );
    // the help sits in the row's first part, the restore button after the control
    const row = (key: string) =>
      web.querySelector(`[id="${key}-help"]`)!.parentElement!.parentElement!;
    const restore = (key: string) =>
      [...row(key).querySelectorAll("button")].find((b) =>
        b.getAttribute("aria-label")?.startsWith("Restore default")
      );
    const reset = restore("web.port")!;
    // a field already at its default has no marker
    expect(restore("web.ui")).toBeUndefined();
    reset.click();
    flushSync();
    expect(web.querySelector<HTMLInputElement>('[id="web.port"]')!.value).toBe("5030");
    [...web.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Save")!.click();
    await vi.waitFor(() => expect(fetcher).toHaveBeenCalled());
    const body = await fetcher.mock.calls[0][0].json();
    expect(body).toMatchObject({ set: {}, unset: ["web.port"] });
    vi.unstubAllGlobals();
  });

  it("keeps a value a read-only file holds, saying why it can't be cleared", async () => {
    const fetcher = vi.fn<(request: Request) => Promise<Response>>(async () =>
      Response.json(written())
    );
    vi.stubGlobal("fetch", fetcher);
    const v = structuredClone(view);
    v.files[0].writable = false;
    (v.values as { analysis: { timeout: string } }).analysis.timeout = "5m";
    v.fields["analysis.timeout"].source = { layer: "file", file: "config.toml" };
    const target = render(
      sections.find((s) => s.id === "analysis")!,
      v
    );
    const reason = "Set in config.toml, so it can't be cleared here";
    const restore = [...target.querySelectorAll("button")].find(
      (b) => b.getAttribute("aria-label") === "Restore default of Time limit per file"
    )!;
    expect(restore.getAttribute("aria-disabled")).toBe("true");
    restore.click();
    flushSync();
    const input = target.querySelector<HTMLInputElement>('[id="analysis.timeout"]')!;
    expect(input.value).toBe("5m");
    // emptied by hand, the apply refuses it under the field and sends nothing
    input.value = "";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    const trigger = [...target.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Apply at runtime")
    )!;
    trigger.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0 }));
    await flush();
    [...document.querySelectorAll<HTMLElement>("[role=menuitem]")]
      .find((i) => i.textContent?.trim() === "Until removed")!
      .click();
    await flush();
    expect(fetcher).not.toHaveBeenCalled();
    expect(target.querySelector('[role="alert"]')?.textContent).toContain(reason);
    vi.unstubAllGlobals();
  });

  it("names the drafts Copy as Nix leaves out as not valid", async () => {
    const copied: string[] = [];
    vi.stubGlobal("navigator", {
      ...navigator,
      clipboard: { writeText: async (text: string) => void copied.push(text) }
    });
    const web = render(sections.find((s) => s.id === "web")!);
    const input = web.querySelector<HTMLInputElement>('[id="web.port"]')!;
    input.value = "lots";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    [...web.querySelectorAll("button")]
      .find((b) => b.textContent?.includes("Copy as Nix"))!
      .click();
    await flush();
    expect(web.textContent).toContain("Left out as not valid: HTTP port");
    expect(copied).toHaveLength(1);
    expect(copied[0]).not.toContain("lots");
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

  it("keeps an edit made while the save of that option runs", async () => {
    let answer!: () => void;
    vi.stubGlobal(
      "fetch",
      vi.fn(
        () =>
          new Promise<Response>(
            (resolve) => (answer = () => resolve(Response.json(written({ applied: ["web.port"] }))))
          )
      )
    );
    const target = document.createElement("div");
    document.body.append(target);
    const props = $state({
      section: sections.find((s) => s.id === "web")!,
      view,
      // the daemon now runs what the save sent
      onchanged: async () => {
        const next = structuredClone(view);
        (next.values as { web: { port: number } }).web.port = 6001;
        props.view = next;
      }
    });
    mounted.push(mount(Section, { target, props }));
    flushSync();
    const port = () => target.querySelector<HTMLInputElement>('[id="web.port"]')!;
    const type = (text: string) => {
      port().value = text;
      port().dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
    };
    type("6001");
    [...target.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Save")!.click();
    await vi.waitFor(() => expect(answer).toBeTypeOf("function"));
    type("6002");
    answer();
    await flush();
    expect(port().value).toBe("6002");
    expect(target.querySelector('[aria-live="polite"]')!.textContent).toContain(
      "1 setting changed"
    );
    vi.unstubAllGlobals();
  });

  it("keeps an edit and says what changed when the options changed since they were read", async () => {
    const asked: unknown[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (request: Request) => {
        asked.push(await request.json());
        return Response.json(
          { error: "The options changed since they were read", detail: "second" },
          { status: 409 }
        );
      })
    );
    const target = document.createElement("div");
    document.body.append(target);
    const props = $state({
      section: sections.find((s) => s.id === "web")!,
      view,
      // another admin saved the same option first
      onchanged: async () => {
        const next = structuredClone(view);
        next.revision = "second";
        (next.values as { web: { port: number } }).web.port = 6005;
        props.view = next;
      }
    });
    mounted.push(mount(Section, { target, props }));
    flushSync();
    const port = () => target.querySelector<HTMLInputElement>('[id="web.port"]')!;
    port().value = "6001";
    port().dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    [...target.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Save")!.click();
    await vi.waitFor(() =>
      expect(toasts.map((t) => t.text)).toContain(
        "HTTP port changed meanwhile. Check your changes and save again."
      )
    );
    expect(asked[0]).toMatchObject({ optionsRevision: "fixture" });
    expect(port().value).toBe("6001");
    expect(target.textContent).toContain("The running value has changed");
    toasts.splice(0);
    vi.unstubAllGlobals();
  });

  it("saves after an apply that is still running, rather than skipping the save", async () => {
    const asked: { path: string; body: unknown }[] = [];
    let answerApply!: () => void;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (request: Request) => {
        const path = new URL(request.url).pathname;
        asked.push({ path, body: await request.json() });
        if (path.endsWith("/options")) await new Promise<void>((done) => (answerApply = done));
        return Response.json(written());
      })
    );
    // the time limit lives in a file that can't be written, so it is applied at runtime
    const v = structuredClone(view);
    v.files.push({ ...v.files[0], name: "ro.toml", writable: false, readOnlyReason: "mounted" });
    v.fields["analysis.timeout"].file = "ro.toml";
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(Section, {
      target,
      props: {
        section: sections.find((s) => s.id === "analysis")!,
        view: v,
        onchanged: async () => {}
      }
    });
    mounted.push(component);
    flushSync();
    const type = (key: string, text: string) => {
      const input = target.querySelector<HTMLInputElement>(`[id="${key}"]`)!;
      input.value = text;
      input.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
    };
    type("analysis.timeout", "5m");
    type("analysis.max_full_minutes", "20");
    const trigger = [...target.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Apply at runtime")
    )!;
    trigger.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0 }));
    await flush();
    [...document.querySelectorAll<HTMLElement>("[role=menuitem]")]
      .find((i) => i.textContent?.trim() === "Until removed")!
      .click();
    await flush();
    expect(asked.map((a) => a.path)).toEqual(["/api/v0/options"]);

    // the page's Save all, while the apply runs
    const saved = (component as unknown as { save: () => Promise<unknown> }).save();
    answerApply();
    await saved;
    expect(asked.map((a) => a.path)).toEqual(["/api/v0/options", "/api/v0/options/file"]);
    expect(asked[1].body).toMatchObject({ set: { "analysis.max_full_minutes": 20 } });
    vi.unstubAllGlobals();
  });
});
