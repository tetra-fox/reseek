import { api, byUser } from "#lib/api/client.js";
import { getFileName, plural } from "#lib/format.js";
import { call } from "#lib/request.js";

/** The one dialog of the page that sends our files to a user: what it sends, while it is open */
class Sending {
  files = $state.raw<string[]>([]);
  open = $state(false);

  /** Asks whom to send `files` to, each a path as other users see it in our shares */
  ask(files: string[]) {
    this.files = files;
    this.open = true;
  }
}

export const sending = new Sending();

/**
 * Text set apart from the sentence around it, so a name in a right-to-left script keeps its
 * place in it: between the first-strong isolate and the pop directional isolate characters
 */
const isolated = (text: string) => `\u2068${text}\u2069`;

/** What sending files came to, in one line */
export const sentText = (username: string, queued: string[], failed: { message: string }[]) => {
  const to = isolated(username);
  const head = queued.length
    ? queued.length === 1
      ? `${isolated(getFileName(queued[0]))} will be offered to ${to}`
      : `${plural(queued.length, "file", "files")} will be offered to ${to}`
    : `Nothing was sent to ${to}`;
  return failed.length
    ? `${head}. ${plural(failed.length, "file", "files")} not sent: ${failed[0].message}`
    : head;
};

/** Offers `files` to `username`, queued as uploads they did not ask for */
export const sendFiles = (username: string, files: string[]) =>
  call(() =>
    api.POST("/api/v0/transfers/uploads/{username}", {
      params: byUser(username),
      body: { files }
    })
  );
