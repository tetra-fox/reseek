<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import EyeOffIcon from "@lucide/svelte/icons/eye-off";

  import * as Menu from "#lib/components/ui/menu/menu.js";
  import UserMenuItems from "#lib/components/users/UserMenuItems.svelte";

  type Props = {
    name: string;
    /** the items name the user, in a menu that is about one of their files */
    named?: boolean;
    /** the folder the user was shown with, which searching their shares starts from */
    folder?: string;
    /** downloads every file of theirs the results hold */
    ondownload?: () => void;
    ondismiss: () => void;
  };

  let { name, named = false, folder, ondownload, ondismiss }: Props = $props();
</script>

<!-- a result's user: their shares, a search in them, all their files, or out of the results -->
<UserMenuItems {name} {named} {folder} only={["shares", "search"]} />
{#if ondownload}
  <Menu.Item onSelect={ondownload}
    ><DownloadIcon />{named ? `Download all from ${name}` : "Download all"}</Menu.Item>
{/if}
<Menu.Item onSelect={ondismiss}
  ><EyeOffIcon />{named ? `Hide results from ${name}` : "Hide result"}</Menu.Item>
