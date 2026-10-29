import { api, type Schemas } from "#lib/api/client.js";
import type { Root } from "#lib/localfiles.js";
import { call, type Outcome } from "#lib/request.js";
import { encodePath } from "#lib/transfers/content.js";

export type Report = Schemas["AudioAnalysis"];
export type Layout = Schemas["SpectrogramLayout"];
export type Guide = Schemas["GuideLine"];
export type Job = Schemas["AnalysisJob"];
export type Judgment = Schemas["Judgment"];
export type Scale = Layout["scale"];

/** The text shown with every spectrogram and every reading */
export const DISCLAIMER =
  "This is a tool to help you judge, not proof. Content can stop short of the top of the graph for honest reasons: a test tone, speech, an old master or a vinyl rip all do. And a careful transcode can reach the top. Listen, compare, and decide for yourself.";

/** The file an analysis is about: one under a root, or a finished download's */
export type Target =
  | { kind: "file"; root: Root; path: string }
  | { kind: "download"; username: string; id: string };

/** The route of a target's analysis, from the daemon's root */
export const analysisRoute = (t: Target) =>
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

/** A target's report, null when it has none yet */
export const loadReport = async (t: Target): Promise<Outcome<Report | null>> => {
  const outcome = await call(() =>
    t.kind === "file"
      ? api.GET("/api/v0/files/{root}/files/{path}/analysis", {
          params: { path: { root: t.root, path: encodePath(t.path) } }
        })
      : api.GET("/api/v0/transfers/downloads/{username}/{id}/analysis", {
          params: { path: { username: t.username, id: t.id } }
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

/** Records the user's judgment of a target, or takes it away with `null` */
export const judge = (t: Target, verdict: Judgment | null, note: string) => {
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
