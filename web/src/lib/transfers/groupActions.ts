import { goto } from "$app/navigation";
import BanIcon from "@lucide/svelte/icons/ban";
import FileArchiveIcon from "@lucide/svelte/icons/file-archive";
import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
import FolderSearchIcon from "@lucide/svelte/icons/folder-search";
import ListChecksIcon from "@lucide/svelte/icons/list-checks";
import PauseIcon from "@lucide/svelte/icons/pause";
import PlayIcon from "@lucide/svelte/icons/play";
import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
import Trash2Icon from "@lucide/svelte/icons/trash-2";
import UserXIcon from "@lucide/svelte/icons/user-x";
import XIcon from "@lucide/svelte/icons/x";
import type { Component } from "svelte";

import { analyzableDownload, analyzeBatch, downloadCandidate } from "#lib/analysis/batch.js";
import { analyzerOffered } from "#lib/analysis/marks.svelte.js";
import { root } from "#lib/api/client.js";
import { confirmation } from "#lib/feedback/confirm.svelte.js";
import { toast } from "#lib/feedback/toast.svelte.js";
import { FILE_ACTIONS as A } from "#lib/files/actions.js";
import { folderLinks, remoteFolder } from "#lib/files/links.js";
import { plural, saveUrl } from "#lib/format.js";
import { href } from "#lib/nav.js";
import { session } from "#lib/session.svelte.js";

import { act, folderOf, openFolder } from "./actions";
import { PAUSE_WARNING } from "./bulk";
import { encodePath } from "./content";
import type { Group } from "./grid/view";
import { type Direction, isCancelable, isRetryable, isSucceeded } from "./states";
import type { Transfers } from "./transfers.svelte";

/** One of a group's actions. `text` shows its label beside the icon on a wide screen */
export type GroupAction = {
  label: string;
  icon: Component;
  run: () => void;
  text?: boolean;
};

/** What a group's actions ask of the list they are in */
export type GroupHandlers = {
  /** removes the group's transfers, asking first for many or running ones */
  onremove: () => void;
  /** cancels the group's transfers, asking first when any is moving bytes */
  oncancel: () => void;
  /** blocks the user, asking first; with `deny` their unfinished uploads are canceled first */
  onblock: (deny: boolean) => void;
};

/**
 * The actions that apply to a group of transfers now, for its line's buttons and its menus. Read
 * inside a reactive context, the list follows the group's transfers and the session
 */
export const groupActions = (
  group: Group,
  store: Transfers,
  direction: Direction,
  { onremove, oncancel, onblock }: GroupHandlers
): GroupAction[] => {
  const write = session.can("ReadWrite");
  const list = group.transfers;
  const local = direction === "download" ? folderOf(list) : null;
  const folderActions = direction === "download" && group.kind === "folder";
  const userUploads = write && direction === "upload" && group.kind === "user";
  // the job a group of what was queued together is, none for the downloads of no job
  const jobId = group.kind === "job" ? group.key.replace(/^job:/, "") : null;
  const job = jobId && jobId !== "none" ? jobId : null;
  const pausable = list.filter((t) => isCancelable(t.state) && !t.paused);
  const paused = list.filter((t) => t.paused && !t.state.includes("Completed"));
  const retryable = list.filter((t) => isRetryable(t.state));

  // saves the group's local folder as a zip; only finished files are in it
  const zip = () => {
    if (!local) return;
    if (list.some((t) => !isSucceeded(t.state)))
      toast("Only finished files are included in the zip");
    const name = local.split("/").pop() || "downloads";
    saveUrl(
      `${root()}/api/v0/files/downloads/directories/${encodePath(local)}/archive?name=${encodeURIComponent(name)}`
    );
  };

  const all: (GroupAction & { shown: boolean })[] = [
    {
      label: "Show the job",
      icon: ListChecksIcon,
      shown: job !== null,
      run: () => void goto(href(`/jobs?job=${job}`)),
      text: true
    },
    {
      label: "Pause the group",
      icon: PauseIcon,
      shown: write && direction === "download" && pausable.length > 0,
      run: () =>
        confirmation.ask({
          title: `Pause ${plural(pausable.length, "download", "downloads")}?`,
          body: PAUSE_WARNING,
          confirm: "Pause",
          run: () =>
            void act(
              store,
              "pause",
              pausable.map((t) => t.id)
            )
        })
    },
    {
      label: "Resume the group",
      icon: PlayIcon,
      shown: write && direction === "download" && paused.length > 0,
      run: () =>
        void act(
          store,
          "resume",
          paused.map((t) => t.id)
        )
    },
    {
      label: "Retry the group",
      icon: RotateCwIcon,
      shown: write && direction === "download" && retryable.length > 0,
      run: () =>
        void act(
          store,
          "retry",
          retryable.map((t) => t.id)
        )
    },
    {
      label: "Cancel the group",
      icon: XIcon,
      shown: write && list.some((t) => isCancelable(t.state)),
      run: oncancel
    },
    { label: "Remove the group", icon: Trash2Icon, shown: write, run: onremove },
    {
      label: "Download more from this folder",
      icon: FolderSearchIcon,
      shown: folderActions,
      run: () =>
        void goto(
          group.folder
            ? folderLinks(group.username, group.folder).browse
            : remoteFolder(group.username, list[0].filename).browse
        )
    },
    {
      label: "Open folder",
      icon: FolderOpenIcon,
      shown: folderActions && local !== null,
      run: () => openFolder(list)
    },
    // files directly in the downloads directory have no folder to zip
    { label: "Download folder", icon: FileArchiveIcon, shown: folderActions && !!local, run: zip },
    {
      label: A.analyze.label,
      icon: A.analyze.icon,
      shown:
        write && direction === "download" && analyzerOffered() && list.some(analyzableDownload),
      run: () => void analyzeBatch(list.map(downloadCandidate), `${group.label} has no downloads`)
    },
    {
      label: "Deny queued uploads",
      icon: UserXIcon,
      shown: userUploads,
      run: () =>
        void act(
          store,
          "cancel",
          list.filter((t) => t.state.includes("Queued")).map((t) => t.id)
        ),
      text: true
    },
    {
      label: `Block ${group.username}`,
      icon: BanIcon,
      shown: userUploads,
      run: () => onblock(false)
    }
  ];
  return all.filter((a) => a.shown);
};
