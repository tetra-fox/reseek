import { chatLayout } from "./prefs.svelte";

/** A column's least, default and greatest width, in rem so they follow the browser's text size */
type Limits = { min: number; width: number; max: number };

/** The chat list. At its least a name, a time and a preview still read */
const LIST: Limits = { min: 15, width: 18, max: 28 };
/** A room's side panel. At its least a member's name and figures still read */
const SIDE: Limits = { min: 13, width: 16, max: 28 };
/** What the messages keep between the columns, in rem */
const MESSAGES = 20;

const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));

/**
 * A column in pixels at `rem` pixels to the rem: its least and greatest width, the width a double
 * click goes back to, and the width it shows at, the one `wanted` as far as `room`, the pixels left
 * for it, allows
 */
export const column = (limits: Limits, wanted: number | null, room: number, rem: number) => {
  const min = Math.round(limits.min * rem);
  const max = Math.round(clamp(room, min, limits.max * rem));
  const initial = clamp(Math.round(limits.width * rem), min, max);
  return { min, max, initial, width: Math.round(clamp(wanted ?? initial, min, max)) };
};

/** The pixels of a rem at the browser's text size */
const rem = () => parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;

/**
 * The chat screen's columns. The messages keep their room first, then the list, so a list grown or
 * a window narrowed takes from the side panel down to its least, and gives it back
 */
class ChatColumns {
  /** the chat screen's width in pixels, 0 until measured */
  total = $state(0);
  /** a wide screen shows the list beside the chat, a narrow one in a sheet */
  listShown = $state(false);
  /** a room's side panel is beside the messages */
  sideShown = $state(false);
  /** the width the list was dragged to, in pixels */
  list = $state(chatLayout.listWidth);
  /** the width the side panel was dragged to, in pixels */
  side = $state(chatLayout.sideWidth);

  /** what the messages leave the columns, without limit until the screen is measured */
  get #room() {
    return this.total ? this.total - MESSAGES * rem() : Infinity;
  }

  get listColumn() {
    const side = this.sideShown ? SIDE.min * rem() : 0;
    return column(LIST, this.list, this.#room - side, rem());
  }

  get sideColumn() {
    const list = this.listShown ? this.listColumn.width : 0;
    return column(SIDE, this.side, this.#room - list, rem());
  }
}

export const columns = new ChatColumns();
