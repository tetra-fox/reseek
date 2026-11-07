import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";

import Flag from "./Flag.svelte";

describe("a country's flag", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    document.body.innerHTML = "";
  });

  const show = (code: string | null | undefined) => {
    component = mount(Flag, { target: document.body, props: { code } });
    return document.body.querySelector("img");
  };

  it.each(["NL", "nl", "Nl"])("is an SVG image named after the country for %s", (code) => {
    const image = show(code)!;
    expect(image.getAttribute("src")).toMatch(/nl.*\.svg/);
    expect(image.alt).toBe("Netherlands");
    // nothing a system font has to draw, which Windows does as two letters
    expect(document.body.textContent?.trim()).toBe("");
  });

  // XX is the package's placeholder, which is a file but no country
  it.each([null, undefined, "", "XX", "ZZ", "N", "NLD", "-", "12", "__"])(
    "shows nothing for %s",
    (code) => {
      expect(show(code)).toBeNull();
      expect(document.body.textContent?.trim()).toBe("");
    }
  );
});
