import { goto } from "$app/navigation";

import { app } from "#lib/app.svelte.js";
import { copyText } from "#lib/feedback/clipboard.svelte.js";
import { toast } from "#lib/feedback/toast.svelte.js";
import { enqueue } from "#lib/files/downloads.js";
import { downloadFile } from "#lib/format.js";
import { href } from "#lib/nav.js";
import { blocking } from "#lib/users/block.svelte.js";

import type { BulkAction } from "./bulk";
import { contentUrl, localFolder } from "./content";
import { isCancelable, type Transfer } from "./states";
import { bump, downloads, type Transfers, uploads } from "./transfers.svelte";

/** The store of a transfer's direction */
export const storeOf = (t: Transfer): Transfers => (t.direction === "Upload" ? uploads : downloads);

/** Runs a bulk action on some transfers of one direction and toasts what it did */
export const act = async (store: Transfers, action: BulkAction, ids: string[]) => {
  if (!ids.length) return;
  const outcome = await store.bulk(action, ids);
  toast(outcome.ok ? outcome.data.text : outcome.error, outcome.ok ? "info" : "error");
};

/** Opens a finished download in a new tab, where the browser shows, plays or saves it */
export const open = async (t: Transfer) => {
  // the tab opens now, while the click still counts as the user's, and is pointed at the file
  const tab = window.open("about:blank", "_blank");
  const url = await contentUrl(t, "inline");
  if (!url.ok) {
    tab?.close();
    toast(url.error, "error");
    return;
  }
  if (tab) tab.location.href = url.data;
};

/** Saves a finished download to this device */
export const saveToDevice = async (t: Transfer) => {
  const url = await contentUrl(t, "attachment");
  if (url.ok) window.location.assign(url.data);
  else toast(url.error, "error");
};

/** The folder a finished download is in under the downloads directory, null when it isn't */
export const folderOf = (transfers: Transfer[]) =>
  localFolder(transfers, app.state?.directories.downloads ?? null);

/** Shows the local folder of some finished downloads on the Files screen */
export const openFolder = (transfers: Transfer[]) => {
  const folder = folderOf(transfers);
  if (folder === null) return toast("None of these downloads has finished", "error");
  void goto(href(`/system/files?root=downloads&path=${encodeURIComponent(folder)}`));
};

/** Downloads the file again as a new download in a batch of its own */
export const redownload = async (t: Transfer) => {
  const result = await enqueue({
    username: t.username,
    files: [{ filename: t.filename, size: t.size }]
  });
  toast(result.text, result.kind === "done" ? "info" : "error");
};

export const copyPath = (path: string) => void copyText(path);

/** Why a download's place in the queue is not known, from the last failed ask */
export const queueFailure = (error: string) => `Could not get the queue position: ${error}`;

/**
 * Asks a queued download's place of the peer, toasting why when the peer can't say; the row's
 * button keeps the reason as its tooltip
 */
export const queuePosition = async (t: Transfer) => {
  const outcome = await downloads.placeInQueue(t);
  if (!outcome.ok) toast(queueFailure(outcome.error), "error");
};

export const bumpUpload = async (t: Transfer) => {
  const outcome = await bump(t);
  toast(
    outcome.ok ? "Moved to the front of the group queue" : outcome.error,
    outcome.ok ? "info" : "error"
  );
};

/** Cancels every upload to a user that hasn't finished, then asks to block them from uploads */
export const denyAndBlock = async (username: string) => {
  const ids = uploads.list.rows
    .filter((t) => t.username === username && isCancelable(t.state))
    .map((t) => t.id);
  await act(uploads, "cancel", ids);
  blocking.ask(username, ["uploads"]);
};

/** The selected transfers as a JSON file to save */
export const exportJson = (transfers: Transfer[]) =>
  downloadFile(JSON.stringify(transfers, null, 2), "transfers.json", "application/json");
