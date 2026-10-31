<script lang="ts">
  import InfoIcon from "@lucide/svelte/icons/info";
  import type { Snippet } from "svelte";

  import * as Menu from "#lib/components/ui/menu/menu.js";

  import RemoteFileItems from "./RemoteFileItems.svelte";

  type Props = {
    /** the file's path as it is shared, and whose share it is in */
    path: string;
    username: string;
    /** enqueues the file, where a row can be downloaded */
    ondownload?: () => void;
    /** opens the inspector on the file; without it the menu has no Details */
    ondetails?: () => void;
    /** the element that opens the menu, which spreads `props` */
    trigger: Snippet<[{ props: Record<string, unknown> }]>;
  };

  let { path, username, ondownload, ondetails, trigger }: Props = $props();
</script>

<!-- the actions every remote file's row offers, wherever the file is listed -->
<Menu.LazyRoot {trigger}>
  <Menu.Content align="end" class="min-w-48">
    <RemoteFileItems {path} {username} {ondownload} />
    {#if ondetails}
      <Menu.Separator />
      <Menu.Item onSelect={ondetails}><InfoIcon />Details</Menu.Item>
    {/if}
  </Menu.Content>
</Menu.LazyRoot>
