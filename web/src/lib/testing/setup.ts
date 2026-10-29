import { installMedia } from "./media";

// jsdom lays nothing out and has no scrollIntoView; lists call it to keep the focused row in view
Element.prototype.scrollIntoView = function () {};
// nor ResizeObserver, which `bind:clientWidth` and the message list measuring its lines use
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
// and gives every box no height, so a virtual list would draw no lines: its scroll box, a grid, tree
// treegrid or log, reports the height of a screen
const offsetHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight")!;
Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
  configurable: true,
  get(this: HTMLElement) {
    return ["grid", "tree", "treegrid", "log"].includes(this.getAttribute("role") ?? "")
      ? 600
      : offsetHeight.get!.call(this);
  }
});
// nor matchMedia, which reduced motion and the narrow layout are read with
installMedia();
