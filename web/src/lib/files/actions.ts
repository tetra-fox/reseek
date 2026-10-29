import AudioLinesIcon from "@lucide/svelte/icons/audio-lines";
import CopyIcon from "@lucide/svelte/icons/copy";
import DownloadIcon from "@lucide/svelte/icons/download";
import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
import LinkIcon from "@lucide/svelte/icons/link";
import ListIcon from "@lucide/svelte/icons/list";
import PlayIcon from "@lucide/svelte/icons/play";
import SaveIcon from "@lucide/svelte/icons/save";

/**
 * What every file and folder menu calls its actions, with their icons, so a file reads the same
 * wherever it is listed
 */
export const FILE_ACTIONS = {
  download: { label: "Download", icon: DownloadIcon },
  open: { label: "Open", icon: ExternalLinkIcon },
  preview: { label: "Preview", icon: PlayIcon },
  analyze: { label: "Analyze", icon: AudioLinesIcon },
  save: { label: "Save to this device", icon: SaveIcon },
  copyPath: { label: "Copy path", icon: CopyIcon },
  copyLocalPath: { label: "Copy local path", icon: CopyIcon },
  copyLink: { label: "Copy slsk:// link", icon: LinkIcon },
  browseFolder: { label: "Browse this folder", icon: FolderOpenIcon },
  allFiles: { label: "View all files in this folder", icon: ListIcon }
} as const;
