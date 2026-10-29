import { type Judgment, JUDGMENTS, type Report, VERDICTS } from "./analysis";

/** What a list shows of a file's analysis: the user's judgment, or else the analyzer's reading */
export type Mark = {
  verdict: Report["verdict"];
  /** the encoder profile the analyzer matched, such as `mp3 ~128 kbps` */
  profile: string | null;
  judgment: Judgment | null;
  note: string | null;
  /** analyzed by an older version, whose report this one cannot show */
  stale: boolean;
};

/** The mark of a report */
export const markOf = (r: Report): Mark => ({
  verdict: r.verdict,
  profile: r.encoderGuess?.profile ?? null,
  judgment: r.override?.verdict ?? null,
  note: r.override?.note ?? null,
  stale: r.stale
});

/** What a mark's chip says when it is pointed at */
export const markHint = (m: Mark) => {
  if (m.judgment) {
    const marked = `Marked ${JUDGMENTS[m.judgment].toLowerCase()}`;
    return m.note ? `${marked}: ${m.note}` : marked;
  }
  if (m.stale) return "Analyzed by an older version of reseek";
  const reading = VERDICTS[m.verdict].toLowerCase();
  return `Analyzer reading: ${reading}${m.profile ? `, ${m.profile}` : ""}`;
};

/** A filter over files by their analysis; `reading:` ones go by the analyzer's verdict */
export type MarkFilter =
  | "fake"
  | "genuine"
  | "analyzed"
  | "unanalyzed"
  | `reading:${Report["verdict"]}`;

/** The filters offered wherever files are listed */
export const MARK_FILTERS: { id: MarkFilter; label: string }[] = [
  { id: "fake", label: "Marked fake" },
  { id: "analyzed", label: "Analyzed" },
  { id: "unanalyzed", label: "Not analyzed" }
];

/** The filters by the analyzer's reading, offered on the Files screen */
export const READING_FILTERS: { id: MarkFilter; label: string }[] = (
  Object.keys(VERDICTS) as Report["verdict"][]
).map((v) => ({ id: `reading:${v}`, label: `Analyzer: ${VERDICTS[v].toLowerCase()}` }));

/** Every filter the Files screen offers */
export const FILE_FILTERS: { id: MarkFilter; label: string }[] = [
  ...MARK_FILTERS,
  { id: "genuine", label: "Marked genuine" },
  ...READING_FILTERS
];

/**
 * Whether a file passes a filter, given its mark if it has been analyzed and whether the analyzer
 * is offered for its kind of file; a file the analyzer is not offered for is never "not analyzed"
 */
export const passes = (filter: MarkFilter, mark: Mark | undefined, analyzable: boolean) => {
  switch (filter) {
    case "fake":
      return mark?.judgment === "fake";
    case "genuine":
      return mark?.judgment === "legit";
    case "analyzed":
      return mark !== undefined;
    case "unanalyzed":
      return mark === undefined && analyzable;
    default:
      return mark?.verdict === filter.slice("reading:".length);
  }
};

/** The words of the filter language that filter by analysis */
const TOKENS = new Map<string, MarkFilter>([
  ["marked:fake", "fake"],
  ["marked:genuine", "genuine"],
  ["analyzed:yes", "analyzed"],
  ["analyzed:no", "unanalyzed"]
]);

/** The filter a `marked:` or `analyzed:` word of the filter language asks for, if it is one */
export const markToken = (token: string) => TOKENS.get(token);
