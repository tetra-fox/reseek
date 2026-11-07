import AudioLinesIcon from "@lucide/svelte/icons/audio-lines";
import CopyIcon from "@lucide/svelte/icons/copy";
import DownloadIcon from "@lucide/svelte/icons/download";
import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
import FolderDownIcon from "@lucide/svelte/icons/folder-down";
import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
import GitCompareArrowsIcon from "@lucide/svelte/icons/git-compare-arrows";
import LinkIcon from "@lucide/svelte/icons/link";
import ListIcon from "@lucide/svelte/icons/list";
import SaveIcon from "@lucide/svelte/icons/save";

/**
 * What every file and folder menu calls its actions, with their icons, so a file reads the same
 * wherever it is listed
 */
export const FILE_ACTIONS = {
  download: { label: "Download", icon: DownloadIcon },
  downloadFolder: { label: "Download this folder", icon: FolderDownIcon },
  open: { label: "Open", icon: ExternalLinkIcon },
  analyze: { label: "Analyze", icon: AudioLinesIcon },
  openAnalysis: { label: "Open in Analysis", icon: AudioLinesIcon },
  compare: { label: "Compare", icon: GitCompareArrowsIcon },
  save: { label: "Save to this device", icon: SaveIcon },
  copyPath: { label: "Copy path", icon: CopyIcon },
  copyLocalPath: { label: "Copy local path", icon: CopyIcon },
  copyLink: { label: "Copy slsk:// link", icon: LinkIcon },
  browseFolder: { label: "Browse this folder", icon: FolderOpenIcon },
  allFiles: { label: "View all files in this folder", icon: ListIcon }
} as const;
