import type { Schemas } from "#lib/api/client.js";
import { toast } from "#lib/feedback/toast.svelte.js";

import { fetchFolder } from "./remote";

/**
 * The dialog listing every file under a remote folder, asked of its user so files a search
 * missed are in: the folder being asked for, and the one shown
 */
export class AllFiles {
  /** the folder asked for and not answered yet, as `username\0path` */
  asking = $state<string | null>(null);
  shown = $state.raw<{ username: string; folder: string; files: Schemas["File"][] } | null>(null);
  open = $state(false);

  /** Whether `folder` of `username` is being asked for */
  isAsking = (username: string, folder: string) => this.asking === `${username}\u0000${folder}`;

  /** Asks for the files and opens the dialog on them, or says why it could not */
  show = async (username: string, folder: string) => {
    this.asking = `${username}\u0000${folder}`;
    const outcome = await fetchFolder(username, folder);
    this.asking = null;
    if (!outcome.ok) {
      toast(`Could not load the folder: ${outcome.error}`, "error");
      return;
    }
    this.shown = { username, folder, files: outcome.data.flatMap((d) => d.files) };
    this.open = true;
  };
}
