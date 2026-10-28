import { api, root, type Schemas } from "#lib/api/client.js";
import type { Root } from "#lib/localfiles.js";
import { call, type Outcome } from "#lib/request.js";
import { encodePath } from "#lib/transfers/content.js";

export type Report = Schemas["AudioAnalysis"];
export type Layout = Schemas["SpectrogramLayout"];
export type Guide = Schemas["GuideLine"];
export type Job = Schemas["AnalysisJob"];
export type Spectrum = Schemas["WindowSpectrum"];
export type Judgement = Schemas["Judgement"];
export type Scale = Layout["scale"];

/** The file an analysis is about: one under a root, or a finished download's */
export type Target =
  | { kind: "file"; root: Root; path: string }
  | { kind: "download"; username: string; id: string };

/** The route of a target's analysis, without the API prefix */
const route = (t: Target) =>
  t.kind === "file"
    ? `/api/v0/files/${t.root}/files/${encodePath(t.path)}/analysis`
    : `/api/v0/transfers/downloads/${encodeURIComponent(t.username)}/${t.id}/analysis`;

/** A key that names the same file however the target names it, for one store of reports */
export const targetKey = (t: Target) =>
  t.kind === "file" ? `file:${t.root}:${t.path}` : `download:${t.id}`;

/** Whether a report or a job is about the file a target names */
export const names = (
  t: Target,
  file: { root: Root; relativePath: string; transferId?: string | null }
) =>
  t.kind === "file"
    ? file.root === t.root && file.relativePath === t.path
    : file.transferId === t.id;

/** The URL of a target's spectrogram image on `scale`, with or without the guide lines drawn in */
export const imageUrl = (t: Target, guides: boolean, scale: Scale) =>
  `${root()}${route(t)}/spectrogram.png?guides=${guides}&scale=${scale}`;

/** A target's report with its spectrogram layout on `scale`, null when it has none yet */
export const loadReport = async (
  t: Target,
  scale: Scale | null
): Promise<Outcome<Report | null>> => {
  // without a scale the daemon gives the layout on the option's
  const query = scale ? { scale } : {};
  const outcome = await call(() =>
    t.kind === "file"
      ? api.GET("/api/v0/files/{root}/files/{path}/analysis", {
          params: { path: { root: t.root, path: encodePath(t.path) }, query }
        })
      : api.GET("/api/v0/transfers/downloads/{username}/{id}/analysis", {
          params: { path: { username: t.username, id: t.id }, query }
        })
  );
  // a report is kept after its file is gone, so a 404 means there is none to show
  if (!outcome.ok && outcome.status === 404) return { ok: true, data: null };
  return outcome;
};

/** Queues a target, again with `force` when it has a report */
export const queue = (t: Target, force = false) =>
  call(() =>
    t.kind === "file"
      ? api.POST("/api/v0/files/{root}/files/{path}/analysis", {
          params: { path: { root: t.root, path: encodePath(t.path) }, query: { force } }
        })
      : api.POST("/api/v0/transfers/downloads/{username}/{id}/analysis", {
          params: { path: { username: t.username, id: t.id }, query: { force } }
        })
  );

/** Records the user's judgement of a target, or takes it away with `null` */
export const judge = (t: Target, verdict: Judgement | null, note: string) => {
  if (verdict === null) {
    return call(() =>
      t.kind === "file"
        ? api.DELETE("/api/v0/files/{root}/files/{path}/analysis/override", {
            params: { path: { root: t.root, path: encodePath(t.path) } }
          })
        : api.DELETE("/api/v0/transfers/downloads/{username}/{id}/analysis/override", {
            params: { path: { username: t.username, id: t.id } }
          })
    );
  }
  const body = { verdict, note: note.trim() || null };
  return call(() =>
    t.kind === "file"
      ? api.PUT("/api/v0/files/{root}/files/{path}/analysis/override", {
          params: { path: { root: t.root, path: encodePath(t.path) } },
          body
        })
      : api.PUT("/api/v0/transfers/downloads/{username}/{id}/analysis/override", {
          params: { path: { username: t.username, id: t.id } },
          body
        })
  );
};

/** The spectrum of one window of a target's report, at the rows of `scale` */
export const loadWindow = (t: Target, index: number, scale: Scale) =>
  call(() =>
    t.kind === "file"
      ? api.GET("/api/v0/files/{root}/files/{path}/analysis/windows/{index}", {
          params: { path: { root: t.root, path: encodePath(t.path), index }, query: { scale } }
        })
      : api.GET("/api/v0/transfers/downloads/{username}/{id}/analysis/windows/{index}", {
          params: { path: { username: t.username, id: t.id, index }, query: { scale } }
        })
  );

/** The mel pitch of `hz`, by O'Shaughnessy's formula: 2595 log10(1 + f / 700) */
const mel = (hz: number) => 2595 * Math.log10(1 + hz / 700);

/**
 * Where `hz` falls on the plot, from 0 at its top to 1 at its bottom, on the axis the daemon drew:
 * hertz the same height on a linear axis, octaves on a log one, pitch as heard on a mel one
 */
export const heightOf = (layout: Layout, hz: number): number => {
  const { minHz: min, maxHz: max } = layout;
  const position =
    layout.scale === "log"
      ? Math.log(Math.max(hz, min) / min) / Math.log(max / min)
      : layout.scale === "mel"
        ? (mel(Math.max(hz, min)) - mel(min)) / (mel(max) - mel(min))
        : (hz - min) / (max - min);
  return 1 - Math.min(1, Math.max(0, position));
};

/**
 * The window shown at a point `across` the plot, from 0 at its left edge to 1 at its right: the
 * plot spans the file's whole length, so the point is a time, and the window is the second that
 * holds it, or where a file was read in samples and the time falls between them, the sample
 * whose middle is nearest
 */
export const windowAt = (layout: Layout, across: number, starts: number[]): number => {
  const t = Math.min(1, Math.max(0, across)) * layout.durationSeconds;
  const holding = starts.findIndex((start) => start <= t && t < start + 1);
  if (holding >= 0) return holding;
  let nearest = 0;
  starts.forEach((start, i) => {
    if (Math.abs(start + 0.5 - t) < Math.abs(starts[nearest] + 0.5 - t)) nearest = i;
  });
  return nearest;
};

/** How far across the plot a time falls, from 0 at its left edge to 1 at its right */
export const acrossAt = (layout: Layout, t: number) =>
  layout.durationSeconds > 0 ? Math.min(1, t / layout.durationSeconds) : 0;

/** The frequency scales a spectrogram can be drawn on, as the switch names them */
export const SCALES: { id: Scale; label: string }[] = [
  { id: "linear", label: "Linear" },
  { id: "log", label: "Log" },
  { id: "mel", label: "Mel" }
];

/** The analyser's verdict in words */
export const VERDICTS: Record<Report["verdict"], string> = {
  consistent: "Consistent with its format",
  likely_transcode: "Likely a transcode",
  mixed_sources: "Mixed sources",
  inconclusive: "Inconclusive"
};

/** What each finding beside the verdict means */
export const FLAGS: Record<Report["flags"][number], string> = {
  upsampled: "Upsampled: made at a lower sample rate and converted up",
  bit_padded: "Padded: the samples use fewer bits than the format holds"
};

/** The judgement in words */
export const JUDGEMENTS: Record<Judgement, string> = { legit: "Genuine", fake: "Fake" };
