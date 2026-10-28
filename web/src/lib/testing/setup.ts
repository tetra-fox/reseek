import { installMedia } from "./media";

// jsdom lays nothing out and has no scrollIntoView; lists call it to keep the focused row in view
Element.prototype.scrollIntoView = function () {};
// nor ResizeObserver, which `bind:clientWidth` and the message list measuring its lines use
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
// nor matchMedia, which reduced motion and the narrow layout are read with
installMedia();
