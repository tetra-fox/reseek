import { ReadableStream, TextDecoderStream } from "node:stream/web";

import { installMedia } from "./media";

// under vmThreads, vitest 5.1's jsdom environment (`setupVM`) copies fetch and Response into the
// context but not the web streams: the fakes answer with a ReadableStream and the live stream
// decodes through a TextDecoderStream. TODO: drop once `setupVM` copies them in too
globalThis.ReadableStream ??= ReadableStream as typeof globalThis.ReadableStream;
globalThis.TextDecoderStream ??= TextDecoderStream as typeof globalThis.TextDecoderStream;

// jsdom lays nothing out and has no scrollIntoView; lists call it to keep the focused row in view
Element.prototype.scrollIntoView = function () {};
// nor ResizeObserver, which `bind:clientWidth` and the message list measuring its lines use
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
// nor getAnimations, which `animate:flip` asks of an item leaving a keyed list
Element.prototype.getAnimations ??= function () {
  return [];
};
// and gives every box no height, so a virtual list would draw no lines: its scroll box, a grid,
// tree, treegrid or log, reports the height of a screen
const clientHeight = Object.getOwnPropertyDescriptor(Element.prototype, "clientHeight")!;
Object.defineProperty(Element.prototype, "clientHeight", {
  configurable: true,
  get(this: Element) {
    return ["grid", "tree", "treegrid", "log"].includes(this.getAttribute("role") ?? "")
      ? 600
      : clientHeight.get!.call(this);
  }
});
// nor matchMedia, which reduced motion and the narrow layout are read with
installMedia();
