/** A page of a paged list, from 1, and how many rows a page holds */
export type Place = { page: number; size: number };

/** The page sizes a paged list offers, and the one it shows where the address names none */
export type PageSizes = { sizes: number[]; size: number };

/** The place an address holds in `page` and `per_page`: page 1 and the list's own size without */
export const placeIn = (query: URLSearchParams, { sizes, size }: PageSizes): Place => {
  const asked = Number(query.get("per_page"));
  const page = Math.floor(Number(query.get("page")));
  return {
    page: Number.isFinite(page) && page > 1 ? page : 1,
    size: sizes.includes(asked) ? asked : size
  };
};

/** `query` with `place` written into it, leaving out page 1 and the list's own size */
export const withPlace = (query: URLSearchParams, place: Place, { size }: PageSizes) => {
  const next = new URLSearchParams(query);
  if (place.page > 1) next.set("page", String(place.page));
  else next.delete("page");
  if (place.size !== size) next.set("per_page", String(place.size));
  else next.delete("per_page");
  return next;
};

/** How many pages `total` rows take at `size` a page, at least one */
export const pageCount = (total: number, size: number) => Math.max(1, Math.ceil(total / size));

/**
 * The page numbers a pager shows: the first, the last, and the pages next to `page`, with a gap
 * for each run left out. A gap would stand for one page only shows that page instead
 */
export const pagerItems = (page: number, pages: number): (number | "gap")[] => {
  const shown = new Set([1, pages, page - 1, page, page + 1]);
  const items: (number | "gap")[] = [];
  for (let n = 1; n <= pages; n++) {
    const kept = shown.has(n) || (shown.has(n - 1) && shown.has(n + 1));
    if (kept) items.push(n);
    else if (items.at(-1) !== "gap") items.push("gap");
  }
  return items;
};
