/**
 * A value kept in this browser's localStorage under `key`, written back whenever it changes.
 * `read` makes the value from what was saved, which may be anything or nothing, so a saved value
 * from an older UI or another hand never breaks the page. Storage can be blocked (private
 * windows, some policies); then `read` gets nothing and changes hold until the page reloads.
 * A value saved under `formerKey`, the key's earlier name, is read when the key has none and
 * moves to the key. Module-level only: the value is kept for the life of the page
 */
export const stored = <T extends object>(
  key: string,
  read: (saved: unknown) => T,
  formerKey?: string
): T => {
  let saved: unknown = null;
  try {
    let text = localStorage.getItem(key);
    if (text === null && formerKey) {
      text = localStorage.getItem(formerKey);
      localStorage.removeItem(formerKey);
    }
    saved = JSON.parse(text ?? "null");
  } catch {
    // unreadable or blocked: read() gives the defaults
  }
  const value = $state(read(saved));
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
  return value;
};

/** `saved` as an object to read fields from, an empty one when it is anything else */
export const fields = (saved: unknown): Record<string, unknown> =>
  saved && typeof saved === "object" && !Array.isArray(saved)
    ? (saved as Record<string, unknown>)
    : {};
