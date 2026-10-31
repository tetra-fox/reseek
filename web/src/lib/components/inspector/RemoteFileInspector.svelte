<script lang="ts">
  import LockIcon from "@lucide/svelte/icons/lock";

  import type { Schemas } from "#lib/api/client.js";
  import RemoteFileItems from "#lib/components/files/RemoteFileItems.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import { downloadOf } from "#lib/files/downloads.js";
  import { slskLink } from "#lib/files/slsk.js";
  import {
    formatAttributes,
    formatBytes,
    formatSeconds,
    getDirectoryName,
    getFileName
  } from "#lib/format.js";
  import type { Field } from "#lib/inspector/inspector.svelte.js";

  import Inspector from "./Inspector.svelte";
  import TransferInspector from "./TransferInspector.svelte";

  // a file in someone's share, as search results and the Shares tab list it. Once it has a
  // download the inspector shows that, as the Downloads screen would, with what the list knows of
  // the file
  type Props = {
    file: Schemas["File"] & { locked?: boolean };
    username: string;
    /** what the list knows of the user's offer */
    facts?: Field[];
    /** enqueues the file, where it can be downloaded now */
    ondownload?: () => void;
  };

  let { file, username, facts = [], ondownload }: Props = $props();

  const download = $derived(downloadOf(username, file.filename));
  const listed = $derived<Field[]>([
    ...facts,
    { label: "Quality", value: formatAttributes(file) },
    { label: "Length", value: formatSeconds(file.length) }
  ]);
</script>

{#snippet chips()}
  <Badge><LockIcon aria-hidden="true" />Locked</Badge>
{/snippet}

{#snippet actions()}
  {#if ondownload}
    <Button size="sm" onclick={ondownload}><A.download.icon />{A.download.label}</Button>
  {/if}
{/snippet}

{#snippet menu()}
  <Menu.Content align="end" class="min-w-48">
    <RemoteFileItems path={file.filename} {username} {ondownload} />
  </Menu.Content>
{/snippet}

{#if download}
  <TransferInspector transfer={download} facts={listed} />
{:else}
  <Inspector
    name={getFileName(file.filename)}
    path={file.filename}
    chips={file.locked ? chips : undefined}
    size={formatBytes(file.size)}
    {username}
    {actions}
    {menu}
    details={[{ label: "Folder", value: getDirectoryName(file.filename) }, ...listed]}
    technical={[
      { label: "Remote path", value: file.filename },
      { label: "Link", value: slskLink(username, file.filename) }
    ]} />
{/if}
