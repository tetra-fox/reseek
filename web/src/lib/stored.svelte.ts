/**
 * A value kept in this browser's localStorage under `key`, written back whenever it changes.
 * `read` makes the value from what was saved, which may be anything or nothing, so a saved value
 * from an older UI or another hand never breaks the page. Storage can be blocked (private
 * windows, some policies); then `read` gets nothing and changes hold until the page reloads.
 * Module-level only: the value is kept for the life of the page
 */
export const stored = <T extends object>(key: string, read: (saved: unknown) => T): T => {
  let saved: unknown = null;
  try {
    saved = JSON.parse(localStorage.getItem(key) ?? "null");
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
