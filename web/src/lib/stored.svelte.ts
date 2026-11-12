/** What was saved as JSON, null when nothing readable was */
const parse = (json: string | null): unknown => {
  try {
    return JSON.parse(json ?? "null");
  } catch {
    // unreadable: read() gives the defaults
    return null;
  }
};

/**
 * A value kept in this browser's localStorage under `key`, written back whenever it changes.
 * `read` makes the value from what was saved, which may be anything or nothing, so a saved value
 * from an older UI or another hand never breaks the page. Another tab's write is taken whole as
 * it happens, so this tab's next write carries it rather than an older copy. Storage can be
 * blocked (private windows, some policies); then `read` gets nothing and changes hold until the
 * page reloads. Module-level only: the value is kept for the life of the page
 */
export const stored = <T extends object>(key: string, read: (saved: unknown) => T): T => {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem(key);
  } catch {
    // blocked: read() gives the defaults
  }
  const value = $state(read(parse(saved)));
  $effect.root(() => {
    $effect(() => {
      const json = JSON.stringify(value);
      try {
        localStorage.setItem(key, json);
      } catch {
        // not saved, the value still holds until the page reloads
      }
    });
  });
  // a key of null is every key, cleared at once
  window.addEventListener("storage", (e) => {
    if (e.key !== key && e.key !== null) return;
    const fresh = read(parse(e.newValue)) as Record<string, unknown>;
    const held = value as Record<string, unknown>;
    for (const field of Object.keys(held)) if (!(field in fresh)) delete held[field];
    Object.assign(held, fresh);
  });
  return value;
};

/** `saved` as an object to read fields from, an empty one when it is anything else */
export const fields = (saved: unknown): Record<string, unknown> =>
  saved && typeof saved === "object" && !Array.isArray(saved)
    ? (saved as Record<string, unknown>)
    : {};
