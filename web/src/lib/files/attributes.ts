/**
 * The longest length a peer is believed about, in seconds. Some clients send -1 for a length
 * they don't know, which reads as 4,294,967,295 seconds
 */
const LONGEST_S = 7 * 86_400;

/** The most a peer's figure is believed: past it, it is a negative number read unsigned */
const SIGNED_MOST = 2 ** 31 - 1;

type Attributes = {
  length?: number | null;
  bitRate?: number | null;
  sampleRate?: number | null;
  bitDepth?: number | null;
};

/** The most each of a file's figures is believed */
const MOST: Record<keyof Attributes, number> = {
  length: LONGEST_S,
  bitRate: SIGNED_MOST,
  sampleRate: SIGNED_MOST,
  bitDepth: SIGNED_MOST
};

/**
 * A remote file as the screens keep it: a length of more than a week, and a bitrate, sample rate
 * or bit depth that is a negative number read unsigned, are unknown
 */
export const withKnownAttributes = <F extends Attributes>(file: F): F => {
  const wrong = (Object.keys(MOST) as (keyof Attributes)[]).filter((k) => (file[k] ?? 0) > MOST[k]);
  return wrong.length ? { ...file, ...Object.fromEntries(wrong.map((k) => [k, null])) } : file;
};
