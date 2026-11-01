import { type Schemas, root, sessionGone } from "#lib/api/client.js";
import { type Outcome, reason, UNREACHABLE } from "#lib/request.js";

/** A file sent to the daemon, and how to stop sending it */
export type Sending = {
  name: string;
  /** how much of it has gone, from 0 to 1 */
  sent: number;
  cancel: () => void;
};

/**
 * Sends `file` to the daemon's analysis cache and says how far it is. This is an XHR, as `fetch`
 * cannot report an upload's progress. `done` is never called after `cancel`
 */
export const sendFile = (
  file: File,
  progress: (sent: number) => void,
  done: (outcome: Outcome<Schemas["AnalysisUpload"]>) => void
) => {
  const name = encodeURIComponent(file.name);
  const request = new XMLHttpRequest();
  request.open("POST", `${root()}/api/v0/analysis/uploads?name=${name}`);
  request.responseType = "json";
  request.setRequestHeader("Content-Type", "application/octet-stream");
  request.upload.onprogress = (e) => {
    if (e.lengthComputable) progress(e.loaded / e.total);
  };
  request.onload = () => {
    const { status, response } = request;
    if (status === 401) sessionGone();
    if (status >= 200 && status < 300) done({ ok: true, data: response });
    else done({ ok: false, error: reason(status, response), status, body: response });
  };
  request.onerror = () => done({ ok: false, error: UNREACHABLE, status: 0 });
  request.send(file);
  return () => request.abort();
};
