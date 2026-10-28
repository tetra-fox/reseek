import { EditorView } from "@codemirror/view";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import fixture from "#lib/config/fixtures/view.json";
import { fakeNetwork } from "#lib/testing/network.js";

import RawFile from "./RawFile.svelte";

vi.mock("$app/navigation", () => ({ beforeNavigate: () => {} }));

const flush = async () => {
  for (let i = 0; i < 4; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
};

describe("the raw configuration file", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  const start = async (put: () => unknown) => {
    const view = structuredClone(fixture) as typeof fixture;
    (view.values as Record<string, unknown>).remote_configuration = true;
    for (const file of view.files) file.writable = true;
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/options/file") && method === "GET") {
        return {
          name: "config.toml",
          path: "/etc/reseek/config.toml",
          text: "a = 1\n",
          revision: "sha256:one"
        };
      }
      if (url.pathname.endsWith("/options/file/validate"))
        return { valid: true, errors: [], warnings: [] };
      if (url.pathname.endsWith("/options/file") && method === "PUT") return put();
      return undefined;
    });
    component = mount(RawFile, { target: document.body, props: { view: view as never } });
    await flush();
    const editor = EditorView.findFromDOM(document.querySelector<HTMLElement>(".cm-editor")!)!;
    editor.dispatch({ changes: { from: editor.state.doc.length, insert: "b = 2\n" } });
    flushSync();
    [...document.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Save")!.click();
    await flush();
    return net;
  };

  it("saves the text against the revision it read and says what the reload did", async () => {
    const net = await start(() => ({
      revision: "sha256:two",
      applied: ["b"],
      reconnectRequired: [],
      restartRequired: ["web.port"]
    }));
    const put = net.requests.find((r) => r.method === "PUT");
    expect(put?.body).toEqual({
      text: "a = 1\nb = 2\n",
      revision: "sha256:one",
      file: "config.toml"
    });
    expect(document.body.textContent?.replace(/\s+/g, " ")).toContain(
      "Saved. 1 setting applied live."
    );
    expect(document.body.textContent).toContain("Restart reseek to apply: web.port.");
  });

  it("asks to reload when the file changed on disk", async () => {
    await start(() => ({ status: 409, body: { error: "config file changed on disk" } }));
    expect(document.body.textContent).toContain("changed on disk. Reload it");
    expect(
      [...document.querySelectorAll("button")].some((b) => b.textContent?.trim() === "Reload")
    ).toBe(true);
  });
});
