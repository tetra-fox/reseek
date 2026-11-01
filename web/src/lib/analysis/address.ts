import { href } from "#lib/nav.js";

import type { Target } from "./analysis";

/** The two places a file can be loaded on the Analysis page */
export type SlotId = "a" | "b";

/**
 * A file as the address writes it, `<root>:<path>` or `download:<id>:<username>`, null for a local
 * file, which only this tab can name
 */
export const targetParam = (t: Target): string | null => {
  if (t.kind === "download") return `download:${t.id}:${t.username}`;
  return t.root === "temporary" ? null : `${t.root}:${t.path}`;
};

/** The file a value of the address names, null for anything else */
export const parseTarget = (text: string | null): Target | null => {
  const cut = text?.indexOf(":") ?? -1;
  if (!text || cut === -1) return null;
  const [kind, rest] = [text.slice(0, cut), text.slice(cut + 1)];
  if (kind === "download") {
    const split = rest.indexOf(":");
    if (split <= 0 || split === rest.length - 1) return null;
    return { kind, id: rest.slice(0, split), username: rest.slice(split + 1) };
  }
  if ((kind === "downloads" || kind === "incomplete") && rest) {
    return { kind: "file", root: kind, path: rest };
  }
  return null;
};

/** The link to the Analysis page with these files in its slots */
export const analysisHref = (a: Target | null, b: Target | null) => {
  const query = new URLSearchParams();
  const put = (slot: SlotId, target: Target | null) => {
    const param = target && targetParam(target);
    if (param) query.set(slot, param);
  };
  put("a", a);
  put("b", b);
  const text = query.toString();
  return href(text ? `/analysis?${text}` : "/analysis");
};
