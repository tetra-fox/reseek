import { createContext } from "svelte";

/** The entry the highlight is on, which each entry reports to its box */
export type Highlight = { lit: string | null };

export const [highlightOf, shareHighlight] = createContext<Highlight>();
