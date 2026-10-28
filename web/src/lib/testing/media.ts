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

/** A media query list that is an event target, so a change reaches listeners as a real event */
class FakeList extends EventTarget {
  onchange = null;
  constructor(readonly media: string) {
    super();
  }
  get matches() {
    return [...matching].some((q) => this.media.includes(q));
  }
  addListener(l: (e: Event) => void) {
    this.addEventListener("change", l);
  }
  removeListener(l: (e: Event) => void) {
    this.removeEventListener("change", l);
  }
}

export const installMedia = () => {
  window.matchMedia = (media: string) => {
    const list = new FakeList(media);
    lists.add({
      media,
      fire: () => {
        list.dispatchEvent(Object.assign(new Event("change"), { matches: list.matches, media }));
      }
    });
    return list as unknown as MediaQueryList;
  };
};
