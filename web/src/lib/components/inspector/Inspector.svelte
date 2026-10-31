<script lang="ts">
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import XIcon from "@lucide/svelte/icons/x";
  import type { Snippet } from "svelte";

  import type { Target } from "#lib/analysis/analysis.js";
  import { FileReport } from "#lib/analysis/report.svelte.js";
  import AnalysisSection from "#lib/components/analysis/AnalysisSection.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Username from "#lib/components/users/Username.svelte";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import type { Track } from "#lib/files/player.svelte.js";
  import { type Field, inspector } from "#lib/inspector/inspector.svelte.js";
  import { session } from "#lib/session.svelte.js";

  import Fields from "./Fields.svelte";
  import Section from "./Section.svelte";

  // one file in the inspector, whichever list it came from: what it is and what to do with it,
  // always in view, then the folded sections, those with nothing to show left out
  type Props = {
    name: string;
    /** the full path, shown when the name is pointed at */
    path: string;
    /** the file's state chips */
    chips?: Snippet;
    /** its size, or how much of it has moved */
    size?: string | null;
    /** how far it has moved, while it moves */
    percent?: number | null;
    username?: string;
    /** the main actions, before Analyze */
    actions?: Snippet;
    /** the rest of the file's actions, a menu's content */
    menu?: Snippet;
    details?: Field[];
    /** the file's analysis, for a file the analyzer takes */
    analysis?: { target: Target; track: Track | null } | null;
    history?: Snippet;
    /** raw values, each with a copy button */
    technical?: Field[];
  };

  let {
    name,
    path,
    chips,
    size,
    percent = null,
    username,
    actions,
    menu,
    details = [],
    analysis = null,
    history,
    technical = []
  }: Props = $props();

  const analyzed = new FileReport(() => analysis?.target ?? null);
  const write = $derived(session.can("ReadWrite"));

  /** Keeps focus where it is at a press, so the keys go on moving through the list after one */
  const keepsFocus = (node: HTMLElement) => {
    const hold = (e: MouseEvent) => e.preventDefault();
    node.addEventListener("mousedown", hold);
    return () => node.removeEventListener("mousedown", hold);
  };

  /** Brings the analysis into view when it was asked for */
  const revealed = (node: HTMLElement) => {
    if (inspector.asked) node.scrollIntoView({ block: "nearest" });
  };
</script>

<header class="flex items-center gap-2 px-4 pt-3">
  <h2 class="min-w-0 flex-1 truncate font-semibold" title={path}>{name}</h2>
  {#if menu}
    <Menu.Root>
      <Menu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon-sm" aria-label="More actions"
            ><EllipsisIcon /></Button>
        {/snippet}
      </Menu.Trigger>
      {@render menu()}
    </Menu.Root>
  {/if}
  <Button
    variant="ghost"
    size="icon-sm"
    aria-label="Close details"
    onclick={() => inspector.close()}><XIcon /></Button>
</header>
<div class="flex flex-col gap-2 px-4 pt-1 pb-3 text-sm">
  {#if chips}
    <div class="flex min-w-0 flex-wrap items-center gap-1.5">{@render chips()}</div>
  {/if}
  {#if size}<p class="text-fg-muted tabular-nums">{size}</p>{/if}
  {#if percent !== null}
    <div
      class="h-2 overflow-hidden rounded-full bg-surface-muted"
      role="progressbar"
      aria-label="Progress"
      aria-valuenow={Math.round(percent)}
      aria-valuemin={0}
      aria-valuemax={100}>
      <div class="h-full bg-accent transition-[width]" style:width={`${percent}%`}></div>
    </div>
  {/if}
  {#if username}<p class="min-w-0 truncate"><Username name={username} /></p>{/if}
  <div class="flex flex-wrap gap-inline empty:hidden" {@attach keepsFocus}>
    {@render actions?.()}
    {#if analysis && analyzed.unread && !analyzed.job && write}
      <Button variant="outline" size="sm" onclick={() => void analyzed.analyze(false)}
        ><A.analyze.icon />{A.analyze.label}</Button>
    {/if}
  </div>
</div>
<div class="min-h-0 flex-1 overflow-y-auto">
  {#if details.some((f) => f.value)}
    <Section id="details" title="Details"><Fields fields={details} /></Section>
  {/if}
  {#if analysis}
    <div class="scroll-mt-2" {@attach revealed}>
      <Section id="analysis" title="Analysis">
        <AnalysisSection {analyzed} target={analysis.target} track={analysis.track} {name} />
      </Section>
    </div>
  {/if}
  {#if history}
    <Section id="history" title="History">{@render history()}</Section>
  {/if}
  {#if technical.some((f) => f.value)}
    <Section id="technical" title="Technical"><Fields fields={technical} copy /></Section>
  {/if}
</div>
