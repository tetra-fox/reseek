import { type Judgement, JUDGEMENTS, type Report, VERDICTS } from "./analysis";

/** What a list shows of a file's analysis: the user's judgement, or else the analyser's reading */
export type Mark = {
  verdict: Report["verdict"];
  /** the encoder profile the analyser matched, such as `mp3 ~128 kbps` */
  profile: string | null;
  judgement: Judgement | null;
  note: string | null;
};

/** The mark of a report */
export const markOf = (r: Report): Mark => ({
  verdict: r.verdict,
  profile: r.encoderGuess?.profile ?? null,
  judgement: r.override?.verdict ?? null,
  note: r.override?.note ?? null
});

/** What a mark's chip says when it is pointed at */
export const markHint = (m: Mark) => {
  if (m.judgement) {
    const marked = `You marked this ${JUDGEMENTS[m.judgement].toLowerCase()}`;
    return m.note ? `${marked}: ${m.note}` : marked;
  }
  const reading = VERDICTS[m.verdict].toLowerCase();
  return `The analyser's reading: ${reading}${m.profile ? `, ${m.profile}` : ""}`;
};

/** A filter over files by their analysis; `reading:` ones go by the analyser's verdict */
export type MarkFilter =
  | "fake"
  | "genuine"
  | "analysed"
  | "unanalysed"
  | `reading:${Report["verdict"]}`;

/** The filters offered wherever files are listed */
export const MARK_FILTERS: { id: MarkFilter; label: string }[] = [
  { id: "fake", label: "Marked fake" },
  { id: "analysed", label: "Analysed" },
  { id: "unanalysed", label: "Not analysed" }
];

/** The filters by the analyser's reading, offered on the Files screen */
export const READING_FILTERS: { id: MarkFilter; label: string }[] = (
  Object.keys(VERDICTS) as Report["verdict"][]
).map((v) => ({ id: `reading:${v}`, label: `Analyser: ${VERDICTS[v].toLowerCase()}` }));

/** Every filter the Files screen offers */
export const FILE_FILTERS: { id: MarkFilter; label: string }[] = [
  ...MARK_FILTERS,
  { id: "genuine", label: "Marked genuine" },
  ...READING_FILTERS
];

/**
 * Whether a file passes a filter, given its mark if it has been analysed and whether the analyser
 * is offered for its kind of file; a file the analyser is not offered for is never "not analysed"
 */
export const passes = (filter: MarkFilter, mark: Mark | undefined, analysable: boolean) => {
  switch (filter) {
    case "fake":
      return mark?.judgement === "fake";
    case "genuine":
      return mark?.judgement === "legit";
    case "analysed":
      return mark !== undefined;
    case "unanalysed":
      return mark === undefined && analysable;
    default:
      return mark?.verdict === filter.slice("reading:".length);
  }
};

/** The words of the filter language that filter by analysis */
const TOKENS = new Map<string, MarkFilter>([
  ["marked:fake", "fake"],
  ["marked:genuine", "genuine"],
  ["analysed:yes", "analysed"],
  ["analysed:no", "unanalysed"]
]);

/** The filter a `marked:` or `analysed:` word of the filter language asks for, if it is one */
export const markToken = (token: string) => TOKENS.get(token);
