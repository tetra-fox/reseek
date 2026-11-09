import { api, byUser, type Schemas } from "#lib/api/client.js";
import { call, type Outcome } from "#lib/request.js";
import { encodePath } from "#lib/transfers/content.js";

export type Report = Schemas["AudioAnalysis"];
export type Layout = Schemas["SpectrogramLayout"];
export type Guide = Schemas["GuideLine"];
export type Job = Schemas["AnalysisJob"];
export type Judgment = Schemas["Judgment"];
export type Scale = Layout["scale"];

/** The caution that goes with every reading, in the Analysis page's guide */
export const DISCLAIMER =
  "The analysis is a guide, not proof. Genuine audio can end below the top of the graph, as speech, old masters and vinyl rips often do, and a careful transcode can reach the top. Listen before deciding.";

/** What a file with no report says */
export const NOT_ANALYZED = "Not analyzed yet";

/** What a file whose report an older version wrote says */
export const STALE = "Results from an older version of reseek can't be shown";

/** The file an analysis is about: one under a root, or a finished download's */
export type Target =
  | { kind: "file"; root: Schemas["FileRoot"]; path: string }
  | { kind: "download"; username: string; id: string };

/** The route of a target's analysis, from the daemon's root, and the query it needs */
export const analysisRoute = (t: Target): { path: string; query: Record<string, string> } => {
  if (t.kind === "file") {
    return { path: `/api/v0/files/${t.root}/files/${encodePath(t.path)}/analysis`, query: {} };
  }
  const user = byUser(t.username);
  return {
    path: `/api/v0/transfers/downloads/${encodeURIComponent(user.path.username)}/${t.id}/analysis`,
    query: user.query
  };
};

/** A key that names the same file however the target names it, for one store of reports */
export const targetKey = (t: Target) =>
  t.kind === "file" ? `file:${t.root}:${t.path}` : `download:${t.id}`;

/** Whether a report or a job is about the file a target names */
export const names = (
  t: Target,
  file: { root: Schemas["FileRoot"]; relativePath: string; transferId?: string | null }
) =>
  t.kind === "file"
    ? file.root === t.root && file.relativePath === t.path
    : file.transferId === t.id;

/** A target's report, null when it has none yet */
export const loadReport = async (t: Target): Promise<Outcome<Report | null>> => {
  const outcome = await call(() =>
    t.kind === "file"
      ? api.GET("/api/v0/files/{root}/files/{path}/analysis", {
          params: { path: { root: t.root, path: encodePath(t.path) } }
        })
      : api.GET("/api/v0/transfers/downloads/{username}/{id}/analysis", {
          params: {
            path: { ...byUser(t.username).path, id: t.id },
            query: byUser(t.username).query
          }
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
          params: {
            path: { ...byUser(t.username).path, id: t.id },
            query: { ...byUser(t.username).query, force }
          }
        })
  );

/** Records the user's judgment of a target, or takes it away with `null` */
export const judge = (t: Target, verdict: Judgment | null, note: string) => {
  if (verdict === null) {
    return call(() =>
      t.kind === "file"
        ? api.DELETE("/api/v0/files/{root}/files/{path}/analysis/override", {
            params: { path: { root: t.root, path: encodePath(t.path) } }
          })
        : api.DELETE("/api/v0/transfers/downloads/{username}/{id}/analysis/override", {
            params: {
              path: { ...byUser(t.username).path, id: t.id },
              query: byUser(t.username).query
            }
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
          params: {
            path: { ...byUser(t.username).path, id: t.id },
            query: byUser(t.username).query
          },
          body
        })
  );
};

/** The frequency scales a spectrogram can be drawn on, as the switch names them */
export const SCALES: { id: Scale; label: string }[] = [
  { id: "linear", label: "Linear" },
  { id: "log", label: "Log" },
  { id: "mel", label: "Mel" }
];

/** The analyzer's verdict in words */
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

/** The judgment in words */
export const JUDGMENTS: Record<Judgment, string> = { legit: "Genuine", fake: "Fake" };

/** A line of the cutoff distribution: the lowest and highest cutoff it holds, in Hz, and its windows */
export type CutoffRow = { from: number; to: number; count: number };

/** The share of the windows under which a cutoff is rare */
const RARE = 0.01;

/**
 * The cutoff histogram, ascending by cutoff, as the reading lists it: a run of cutoffs that
 * each hold under 1% of the windows becomes one line spanning them, so a wandering cutoff reads
 * as a few lines rather than one per 500 Hz
 */
export const cutoffRows = (histogram: Report["measures"]["cutoffHzHistogram"]): CutoffRow[] => {
  const total = histogram.reduce((n, [, count]) => n + count, 0);
  const rows: CutoffRow[] = [];
  let lastRare = false;
  for (const [hz, count] of histogram) {
    const rare = count < total * RARE;
    const last = rows.at(-1);
    if (rare && lastRare && last) {
      last.to = hz;
      last.count += count;
    } else {
      rows.push({ from: hz, to: hz, count });
    }
    lastRare = rare;
  }
  return rows;
};

/** `count` of `total` as a whole percentage that never reads 0% or 100% when it is neither */
export const percentOf = (count: number, total: number): string => {
  const percent = Math.round((count / total) * 100);
  if (percent === 0 && count > 0) return "<1%";
  if (percent === 100 && count < total) return ">99%";
  return `${percent}%`;
};
