<script lang="ts">
  import { goto } from "$app/navigation";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import ListChecksIcon from "@lucide/svelte/icons/list-checks";
  import SearchIcon from "@lucide/svelte/icons/search";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { tick, untrack } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import * as Filter from "#lib/components/ui/filter/filter.js";
  import Input from "#lib/components/ui/Input.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { confirmation } from "#lib/feedback/confirm.svelte.js";
  import { plural } from "#lib/format.js";
  import {
    filtered,
    type JobKind,
    KIND_LABELS,
    STARTER_LABELS,
    type StarterKind,
    STATE_FILTERS,
    type StateFilter
  } from "#lib/jobs/jobs.js";
  import { jobs, jobsSeen } from "#lib/jobs/jobs.svelte.js";
  import type { ActiveFilter } from "#lib/lists/filtered.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { href } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";

  import JobRow from "./JobRow.svelte";
  import SchedulesCard from "./SchedulesCard.svelte";

  // every job, the running and paused ones first, then those waiting, then the ended ones newest
  // first, under the schedules; `shown` opens one job's parts, as a link from elsewhere asks
  let { shown = null }: { shown?: string | null } = $props();

  keepCurrent(() => void jobs.load());
  void jobs.load();
  // the page shows every failure while it is open, so the bottom bar's mark clears; the
  // daemon's own time of the last failure is kept, so no clock is compared with another
  $effect(() => {
    const last = app.state?.jobs.lastFailedAt;
    if (last) jobsSeen.at = last;
  });

  const write = $derived(session.can("ReadWrite"));
  const busy = new Busy();

  let kinds = $state<JobKind[]>([]);
  let states = $state<StateFilter[]>([]);
  let starters = $state<StarterKind[]>([]);
  let text = $state("");
  const all = $derived(jobs.list);
  const listed = $derived(filtered(all, { kinds, states, starters, text }));
  const ended = $derived(all.filter((j) => j.state === "ended").length);

  let opened = $state<string | null>(null);
  /** Opens the job `id` and brings its row into view */
  const focus = (id: string) => {
    opened = id;
    void tick().then(() =>
      document.getElementById(`job-${id}`)?.scrollIntoView({ block: "center" })
    );
  };
  // a link to a job opens it once its row is there, which a cold load waits for. The link points
  // once: it leaves the address, so Jobs comes back where it was left rather than to the job
  $effect(() => {
    if (!shown || !jobs.rows.get(shown)) return;
    const id = shown;
    untrack(() => {
      focus(id);
      void goto(href("/jobs"), { replace: true, reset: false });
    });
  });
  const toggle = (id: string) => (opened = opened === id ? null : id);

  const flip = <T>(list: T[], value: T) =>
    list.includes(value) ? list.filter((v) => v !== value) : [...list, value];

  const JOBS = ["job", "jobs"] as const;
  let toolbar = $state<HTMLElement | null>(null);
  const refocus = () => toolbar?.querySelector("input")?.focus();
  const filters = $derived<ActiveFilter[]>([
    ...kinds.map((k) => ({
      id: `kind:${k}`,
      label: KIND_LABELS[k],
      clear: () => (kinds = kinds.filter((v) => v !== k))
    })),
    ...states.map((s) => ({
      id: `state:${s}`,
      label: STATE_FILTERS.find((f) => f.id === s)!.label,
      clear: () => (states = states.filter((v) => v !== s))
    })),
    ...starters.map((s) => ({
      id: `starter:${s}`,
      label: `Started by ${STARTER_LABELS[s].toLowerCase()}`,
      clear: () => (starters = starters.filter((v) => v !== s))
    })),
    ...(text.trim() ? [{ id: "text", label: `"${text.trim()}"`, clear: () => (text = "") }] : [])
  ]);
  const clearAll = () => {
    kinds = [];
    states = [];
    starters = [];
    text = "";
  };

  const clearEnded = () =>
    confirmation.ask({
      title: "Clear finished jobs?",
      body: `${plural(ended, "finished job", "finished jobs")} will be removed. Running and queued jobs stay.`,
      confirm: "Clear",
      run: () => void busy.run(() => jobs.clearEnded(), "Finished jobs cleared")
    });
</script>

<div class="flex flex-col gap-section">
  <SchedulesCard onshow={focus} />

  <div class="flex flex-wrap items-center gap-inline" bind:this={toolbar}>
    <Input
      type="search"
      icon={SearchIcon}
      placeholder="Search jobs"
      aria-label="Search jobs by name"
      wrapperClass="w-full md:w-64"
      bind:value={text} />
    {#snippet menu(
      label: string,
      count: number,
      items: { id: string; label: string; on: boolean; flip: () => void }[]
    )}
      <Menu.Root>
        <Menu.Trigger>
          {#snippet child({ props })}
            <Button {...props} variant="outline" size="sm" class={[count && Filter.FILTERING]}
              >{label}{count ? ` (${count})` : ""}<ChevronDownIcon /></Button>
          {/snippet}
        </Menu.Trigger>
        <Menu.Content align="start">
          {#each items as item (item.id)}
            <Menu.CheckboxItem closeOnSelect={false} checked={item.on} onCheckedChange={item.flip}
              >{item.label}</Menu.CheckboxItem>
          {/each}
        </Menu.Content>
      </Menu.Root>
    {/snippet}
    {@render menu(
      "Kind",
      kinds.length,
      (Object.keys(KIND_LABELS) as JobKind[]).map((k) => ({
        id: k,
        label: KIND_LABELS[k],
        on: kinds.includes(k),
        flip: () => (kinds = flip(kinds, k))
      }))
    )}
    {@render menu(
      "State",
      states.length,
      STATE_FILTERS.map((s) => ({
        id: s.id,
        label: s.label,
        on: states.includes(s.id),
        flip: () => (states = flip(states, s.id))
      }))
    )}
    {@render menu(
      "Started by",
      starters.length,
      (Object.keys(STARTER_LABELS) as StarterKind[]).map((s) => ({
        id: s,
        label: STARTER_LABELS[s],
        on: starters.includes(s),
        flip: () => (starters = flip(starters, s))
      }))
    )}
    {#if write && ended}
      <Button
        class="ml-auto"
        size="sm"
        variant="outline"
        disabled={busy.running}
        onclick={clearEnded}><Trash2Icon />Clear finished</Button>
    {/if}
  </div>

  {#if jobs.loaded}
    <Filter.Summary
      {filters}
      shown={listed.length}
      total={all.length}
      noun={JOBS}
      {refocus}
      onclear={clearAll} />
  {/if}

  {#if jobs.error && !all.length}
    <ErrorSegment caption={jobs.error} retry={() => void jobs.load()} retryLabel="Retry" />
  {:else if !jobs.loaded}
    <Skeleton class="h-48" />
  {:else if !all.length}
    <EmptyState
      icon={ListChecksIcon}
      title="No jobs"
      description="Downloads, share scans, analyses and cleanups appear here" />
  {:else if !listed.length}
    <Filter.Empty {filters} total={all.length} noun={JOBS} {refocus} onclear={clearAll} />
  {:else}
    <!-- the rows share the list's columns, so rows with different actions line up -->
    <ul
      class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 divide-y overflow-hidden rounded-card border bg-surface text-sm md:grid-cols-[auto_minmax(0,2fr)_minmax(0,1fr)_minmax(0,1fr)_auto]">
      {#each listed as job (job.id)}
        <li id={`job-${job.id}`} class="col-span-full grid grid-cols-subgrid list-row">
          <JobRow {job} open={opened === job.id} ontoggle={() => toggle(job.id)} />
        </li>
      {/each}
    </ul>
  {/if}
</div>
