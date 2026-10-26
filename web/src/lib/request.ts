/** What an API call came to: its data, or why it failed in words for a person */
export type Outcome<T> = { ok: true; data: T } | { ok: false; error: string; status: number };

/** The error body every 4xx and 5xx carries */
type ErrorBody = { error?: string | null; detail?: string | null };

/** What openapi-fetch hands back: the body on success, the error body otherwise */
type Reply<T> = { data?: T; error?: unknown; response: Response };

/** The message for a failed request that never got an answer */
export const UNREACHABLE = "Can't reach reseek, check that it's running";

/** Why a request failed: the daemon's own message with its detail when it sent one */
export const reason = (status: number, body?: unknown): string => {
  const { error, detail } = (body && typeof body === "object" ? body : {}) as ErrorBody;
  if (error) return detail ? `${error}: ${detail}` : error;
  if (typeof body === "string" && body) return body;
  if (status === 403) return "Not allowed";
  if (status === 404) return "Not found";
  if (status === 429) return "Too many tries, wait a moment";
  if (status >= 500) return `reseek couldn't answer (HTTP ${status})`;
  return `HTTP ${status}`;
};

/** Runs an openapi-fetch call, telling a request that got no answer from one that was refused */
export const call = async <T>(request: () => Promise<Reply<T>>): Promise<Outcome<T>> => {
  let reply: Reply<T>;
  try {
    reply = await request();
  } catch {
    // fetch rejects only when no response came back at all
    return { ok: false, error: UNREACHABLE, status: 0 };
  }
  const { data, error, response } = reply;
  // a 204 has no body, which openapi-fetch reads as undefined data
  if (response.ok) return { ok: true, data: data as T };
  return { ok: false, error: reason(response.status, error), status: response.status };
};
