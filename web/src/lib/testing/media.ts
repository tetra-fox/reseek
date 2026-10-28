/**
 * The page's media queries for tests, which jsdom lacks. Every query is false until `setMedia`
 * says otherwise, and a change is told to the lists listening, as a browser does
 */
const matching = new Set<string>();
const lists = new Set<{ media: string; fire: () => void }>();

export const setMedia = (query: string, on: boolean) => {
  if (on) matching.add(query);
  else matching.delete(query);
  for (const list of lists) if (list.media.includes(query)) list.fire();
};

export const installMedia = () => {
  window.matchMedia = (media: string) => {
    const listeners = new Set<(e: MediaQueryListEvent) => void>();
    const list = {
      media,
      get matches() {
        return [...matching].some((q) => media.includes(q));
      },
      onchange: null,
      addEventListener: (_: string, l: (e: MediaQueryListEvent) => void) => listeners.add(l),
      removeEventListener: (_: string, l: (e: MediaQueryListEvent) => void) => listeners.delete(l),
      addListener: (l: (e: MediaQueryListEvent) => void) => listeners.add(l),
      removeListener: (l: (e: MediaQueryListEvent) => void) => listeners.delete(l),
      dispatchEvent: () => true
    };
    lists.add({
      media,
      fire: () => {
        for (const l of listeners) l({ matches: list.matches, media } as MediaQueryListEvent);
      }
    });
    return list as unknown as MediaQueryList;
  };
};
