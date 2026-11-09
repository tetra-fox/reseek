<script lang="ts">
  import CopyIcon from "@lucide/svelte/icons/copy";
  import TerminalIcon from "@lucide/svelte/icons/terminal";
  import { Popover } from "bits-ui";

  import Button from "#lib/components/ui/Button.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";

  // an option's environment variable and command-line flag, with a copy button for each, opened
  // by a click or a tap. The popover is made on the first open, so a page of options doesn't
  // build one per row
  type Props = {
    /** the option's label, which the button names */
    label: string;
    envVar: string | null | undefined;
    cliFlag: string | null | undefined;
  };

  let { label, envVar, cliFlag }: Props = $props();

  let button = $state<HTMLButtonElement | null>(null);
  let open = $state(false);
  /** the popover has been opened once, and stays made from then on */
  let made = $state(false);

  const lines = $derived(
    [
      envVar && { text: `${envVar}=…`, name: envVar },
      cliFlag && { text: `${cliFlag} …`, name: cliFlag }
    ].filter((l): l is { text: string; name: string } => !!l)
  );
</script>

<button
  bind:this={button}
  type="button"
  class="flex w-fit items-center gap-1 rounded-badge text-xs text-fg-subtle hover:text-fg"
  aria-label={`Environment variable and flag for ${label}`}
  aria-haspopup="dialog"
  aria-expanded={open}
  onclick={() => {
    made = true;
    open = !open;
  }}>
  <TerminalIcon class="size-3.5" />Environment and flag
</button>
{#if made}
  <Popover.Root bind:open>
    <Popover.Portal>
      <!-- the button is the anchor rather than a trigger, so a click on it toggles the popover
           rather than closing it as a click outside -->
      <Popover.Content
        customAnchor={button}
        side="bottom"
        align="start"
        sideOffset={4}
        onCloseAutoFocus={(e) => {
          e.preventDefault();
          button?.focus();
        }}
        class="z-50 flex w-max max-w-[calc(100vw-1rem)] flex-col gap-1.5 rounded-card border bg-overlay p-3 text-xs shadow-raised outline-none data-[state=closed]:animate-popover-out data-[state=open]:animate-popover-in">
        <p class="text-fg-muted">Environment variable and command-line flag:</p>
        {#each lines as line (line.name)}
          <span class="flex items-center gap-2">
            <code
              class="min-w-0 flex-1 rounded-badge bg-surface-muted px-1.5 py-0.5 font-mono wrap-anywhere"
              >{line.text}</code>
            <Tooltip text={`Copy ${line.name}`}>
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Copy ${line.name}`}
                  onclick={() => void copyText(line.name)}><CopyIcon /></Button>
              {/snippet}
            </Tooltip>
          </span>
        {/each}
      </Popover.Content>
    </Popover.Portal>
  </Popover.Root>
{/if}
