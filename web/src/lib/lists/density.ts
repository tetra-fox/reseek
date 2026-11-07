/** How close the lines of every file list sit, one choice for the whole UI */
export type Density = "comfortable" | "compact";

/** Each density's name as the UI shows it */
export const DENSITY_LABELS: Record<Density, string> = {
  comfortable: "Comfortable",
  compact: "Compact"
};

/**
 * A file list line's height in rem for each density, on a wider screen and on a phone, where a
 * line puts a file's size under its name
 */
export const LINE_REM: Record<Density, { wide: number; phone: number }> = {
  comfortable: { wide: 2.25, phone: 3.25 },
  compact: { wide: 1.75, phone: 2.75 }
};
