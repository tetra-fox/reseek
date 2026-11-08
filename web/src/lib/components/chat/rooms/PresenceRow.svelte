<script lang="ts">
  import LogInIcon from "@lucide/svelte/icons/log-in";
  import LogOutIcon from "@lucide/svelte/icons/log-out";
  import MoonIcon from "@lucide/svelte/icons/moon";
  import ShieldIcon from "@lucide/svelte/icons/shield";
  import SunIcon from "@lucide/svelte/icons/sun";

  import { type Burst, burstLine } from "#lib/chat/rooms/presence.js";
  import LogTime from "#lib/components/ui/LogTime.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { formatCount } from "#lib/format.js";

  // the cells of one line of a room's activity: a burst of the same change by its members
  let { burst }: { burst: Burst } = $props();

  const line = $derived(burstLine(burst));
  /** where the commas and the "and" go: "a, b and c", "a, b and 3 others" */
  const commaAfter = (i: number) =>
    i < line.shown.length - 1 && (line.others > 0 || i < line.shown.length - 2);
  const andBefore = (i: number) => i > 0 && i === line.shown.length - 1 && !line.others;
  const Icon = $derived(
    {
      joined: LogInIcon,
      added: LogInIcon,
      left: LogOutIcon,
      removed: LogOutIcon,
      away: MoonIcon,
      returned: SunIcon,
      operator: ShieldIcon,
      notOperator: ShieldIcon
    }[burst.kind]
  );
</script>

<LogTime at={burst.at} />
<span role="gridcell" class="flex justify-center pt-1">
  <Icon class="size-3.5 text-fg-muted" aria-hidden="true" />
</span>
<!-- a comma goes in its name's box, so a line never starts with one -->
<span role="gridcell" class="min-w-0 wrap-anywhere">
  {#each line.shown as name, i (name)}{#if i > 0}{" "}{/if}{#if andBefore(i)}and{" "}{/if}<span
      class="inline-flex max-w-full min-w-0 align-baseline"
      ><Username {name} card />{#if commaAfter(i)}<span class="shrink-0">,</span>{/if}</span
    >{/each}{#if line.others}{" "}and <Tooltip
      text={burst.names.slice(line.shown.length).join(", ")}
      dense
      spoken
      >{#snippet child({ props })}<span {...props}>{formatCount(line.others)} others</span
        >{/snippet}</Tooltip
    >{/if}
  <span class="text-fg-muted">{line.verb}</span>
</span>
