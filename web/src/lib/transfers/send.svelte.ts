import { api } from "#lib/api/client.js";
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

/** What sending files came to, in one line */
export const sentText = (username: string, queued: string[], failed: { message: string }[]) => {
  const head = queued.length
    ? queued.length === 1
      ? `${getFileName(queued[0])} will be offered to ${username}`
      : `${plural(queued.length, "file", "files")} will be offered to ${username}`
    : `Nothing was sent to ${username}`;
  return failed.length
    ? `${head}. ${plural(failed.length, "file", "files")} not sent: ${failed[0].message}`
    : head;
};

/** Offers `files` to `username`, queued as uploads they did not ask for */
export const sendFiles = (username: string, files: string[]) =>
  call(() =>
    api.POST("/api/v0/transfers/uploads/{username}", {
      params: { path: { username } },
      body: { files }
    })
  );
