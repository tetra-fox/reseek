/**
 * A row's `onpointerenter` that calls `wake` for a mouse or a pen, for a row whose tooltips are
 * dormant until then (`Tooltip`'s `dormant`). A touch shows no tooltip, and waking them would
 * replace the element it landed on before its pointerdown, which the browser then sends nowhere,
 * losing a long press
 */
export const wakeOnPointer = (wake: () => void) => (e: PointerEvent) => {
  if (e.pointerType !== "touch") wake();
};
