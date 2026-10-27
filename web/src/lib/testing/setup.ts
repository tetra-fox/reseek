// jsdom lays nothing out and has no scrollIntoView; lists call it to keep the focused row in view
Element.prototype.scrollIntoView = function () {};
// nor ResizeObserver, which `bind:clientWidth` and `bind:clientHeight` use
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
