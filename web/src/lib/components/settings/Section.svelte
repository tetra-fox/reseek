<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import { api } from "#lib/api/client.js";
  import AddShare from "#lib/components/system/AddShare.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import { editability, fileOf } from "#lib/config/editability.js";
  import {
    type ConfigView,
    controlOf,
    type Draft,
    errorsByKey,
    fromDraft,
    REDACTED,
    toDraft
  } from "#lib/config/fields.js";
  import { fieldMatches } from "#lib/config/find.js";
  import { asNixSettings } from "#lib/config/nix.js";
  import { type Field, type Section, valueAt } from "#lib/config/sections.js";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { CopyButton } from "#lib/feedback/clipboard.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { plural } from "#lib/format.js";
  import { same } from "#lib/lists/keyed.svelte.js";
  import { href } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { UNTIL_RESTART } from "#lib/wording.js";

  import FieldRow from "./FieldRow.svelte";
  import OidcTest from "./OidcTest.svelte";

  type Props = {
    section: Section;
    view: ConfigView;
    /** reloads the view after a write, or after a write was refused because the file changed */
    onchanged: () => Promise<void>;
    /** what the settings search holds: only the options it finds show */
    query?: string;
    /** how many of the section's options differ from what runs, for the page's bar */
    changes?: number;
  };

  let { section, view, onchanged, query = "", changes = $bindable(0) }: Props = $props();

  const fields = $derived(section.fields.filter((f) => f.key in view.fields));
  /** the options the search finds; every one without a search */
  const found = $derived(fields.filter((f) => fieldMatches(f, view.fields[f.key], query)));
  /** Whether `field` is the first of a subheading, and the search finds something under it */
  const opensHeading = (field: Field, i: number) =>
    !!field.heading &&
    field.heading !== fields[i - 1]?.heading &&
    found.some((f) => f.heading === field.heading);
  const controls = $derived(
    Object.fromEntries(fields.map((f) => [f.key, controlOf(f, view.fields[f.key])]))
  );
  const edits = $derived(Object.fromEntries(fields.map((f) => [f.key, editability(view, f.key)])));

  // the running values as the controls hold them
  const running = $derived(
    Object.fromEntries(
      fields.map((f) => [f.key, toDraft(controls[f.key], valueAt(view.values, f.key))])
    ) as Record<string, Draft>
  );
  let loaded: Record<string, Draft> = untrack(() => running);
  let draft = $state<Record<string, Draft>>(untrack(() => ({ ...running })));
  /** keys whose running value changed under an edit in progress */
  let moved = $state<string[]>([]);
  let errors = $state<Record<string, string>>({});
  /** how many writes each key has had, so its control starts afresh after one */
  let written = $state<Record<string, number>>({});

  // a reload (a save here, or a change from elsewhere) takes the new values into fields that
  // weren't edited, so an edit in progress is kept and marked instead of lost
  $effect(() => {
    const next = running;
    untrack(() => {
      if (next === loaded) return;
      const shifted: string[] = [];
      draft = Object.fromEntries(
        Object.entries(next).map(([key, value]) => {
          const edited = !same(draft[key], loaded[key]);
          if (edited && !same(value, loaded[key])) shifted.push(key);
          return [key, edited ? draft[key] : value];
        })
      );
      moved = shifted;
      loaded = next;
    });
  });

  const changed = $derived(fields.filter((f) => !same(draft[f.key], running[f.key])));
  // the page's bar counts every section's changes
  $effect(() => {
    changes = changed.length;
  });

  /** the drafts put back to the default, which a save writes by taking the key out */
  const defaulted = new SvelteSet<string>();
  /** The default of `key` as its control holds it, null for a secret, which shows no value */
  const defaultDraft = (key: string): Draft | null => {
    const info = view.fields[key];
    return info.secret ? null : toDraft(controls[key], info.default ?? null);
  };
  /** whether what runs for `key` is not its default, null where that can't be told */
  const custom = (key: string): boolean | null => {
    const fallback = defaultDraft(key);
    return fallback === null ? null : !same(running[key], fallback);
  };
  const toDefault = (key: string) => {
    const fallback = defaultDraft(key);
    if (fallback === null) return;
    draft[key] = fallback;
    defaulted.add(key);
    written[key] = (written[key] ?? 0) + 1;
  };
  const enabled = (field: Field) =>
    !field.enabledWhen ||
    field.enabledWhen.values.some((v) => {
      const at = draft[field.enabledWhen!.key] ?? valueAt(view.values, field.enabledWhen!.key);
      return same(at, v) || String(at) === String(v);
    });

  const saveable = $derived(fields.some((f) => edits[f.key].mode === "save"));
  const runtimeable = $derived(fields.some((f) => edits[f.key].mode === "runtime"));

  /** The changed fields of one mode as a set and an unset, or null when one doesn't parse */
  const collect = (mode: "save" | "runtime") => {
    const found: Record<string, string> = {};
    const out: { field: Field; set?: unknown; unset?: true }[] = [];
    for (const field of changed.filter((f) => edits[f.key].mode === mode)) {
      // the default put back: the key comes out, so the default applies and stays the default
      if (defaulted.has(field.key) && same(draft[field.key], defaultDraft(field.key))) {
        out.push({ field, unset: true });
        continue;
      }
      const parsed = fromDraft(
        field,
        controls[field.key],
        draft[field.key],
        view.fields[field.key]
      );
      if ("error" in parsed) found[field.key] = parsed.error;
      else out.push({ field, ...parsed });
    }
    errors = found;
    return Object.keys(found).length ? null : out;
  };

  /**
   * Takes what the daemon now runs into the drafts of the keys just written, so the section
   * shows no change for them and a later save doesn't send them again
   */
  const settle = async (keys: string[]) => {
    if (!keys.length) return;
    await onchanged();
    loaded = { ...loaded };
    for (const key of keys) {
      draft[key] = running[key];
      loaded[key] = running[key];
      delete errors[key];
      defaulted.delete(key);
      written[key] = (written[key] ?? 0) + 1;
    }
    moved = moved.filter((key) => !keys.includes(key));
  };

  const saving = new Busy();
  /** Saves the changed options to their files; the page's bar saves every section with it */
  export const save = () =>
    saving.run(async () => {
      const writes = collect("save");
      if (!writes) return { ok: true as const, data: null };
      const byFile = new Map<string, typeof writes>();
      for (const write of writes) {
        const file = fileOf(view, write.field.key);
        byFile.set(file, [...(byFile.get(file) ?? []), write]);
      }
      const done: string[] = [];
      let restart: string[] = [];
      for (const [file, list] of byFile) {
        const outcome = await call(() =>
          api.PATCH("/api/v0/options/file", {
            body: {
              file,
              revision: view.files.find((f) => f.name === file)?.revision ?? null,
              set: Object.fromEntries(
                list.filter((w) => !w.unset).map((w) => [w.field.key, w.set])
              ),
              unset: list.filter((w) => w.unset).map((w) => w.field.key)
            }
          })
        );
        if (!outcome.ok) {
          // the files written before this one keep their change
          await settle(done);
          if (
            outcome.status === 409 &&
            outcome.error.startsWith("The configuration file changed on disk")
          ) {
            if (!done.length) await onchanged();
            return {
              ...outcome,
              error: `${file} changed on disk. Check the values and save again.`
            };
          }
          const detail = (outcome.body as { detail?: string | null } | undefined)?.detail;
          errors = Object.fromEntries(
            errorsByKey(
              detail,
              list.map((w) => w.field.key)
            )
          );
          return outcome;
        }
        done.push(...list.map((w) => w.field.key));
        restart = [...restart, ...outcome.data.restartRequired];
      }
      toast(
        `${section.title}: ${plural(done.length, "setting", "settings")} saved` +
          (restart.length ? `. Restart reseek to apply ${restart.join(", ")}.` : "")
      );
      await settle(done);
      return { ok: true as const, data: null };
    });

  /**
   * How long a runtime change lasts: an expiry for `expiresIn`, "restart" for until reseek stops,
   * null for until removed. Until restart comes first, so a change ends by default with the process
   */
  const EXPIRIES = [
    {
      label: UNTIL_RESTART[0].toUpperCase() + UNTIL_RESTART.slice(1),
      value: "restart",
      words: UNTIL_RESTART
    },
    { label: "For an hour", value: "1h", words: "for an hour" },
    { label: "For a day", value: "1d", words: "for a day" },
    { label: "Until removed", value: null, words: "until removed" }
  ];
  const applying = new Busy();
  const apply = (expiry: (typeof EXPIRIES)[number]) =>
    applying.run(async () => {
      const writes = collect("runtime");
      if (!writes) return { ok: true as const, data: null };
      const outcome = await call(() =>
        api.PATCH("/api/v0/options", {
          body: {
            expiresIn: expiry.value,
            set: Object.fromEntries(
              writes.filter((w) => !w.unset).map((w) => [w.field.key, w.set])
            ),
            unset: writes.filter((w) => w.unset).map((w) => w.field.key)
          }
        })
      );
      if (!outcome.ok) {
        const detail = (outcome.body as { detail?: string | null } | undefined)?.detail;
        errors = Object.fromEntries(
          errorsByKey(
            detail,
            writes.map((w) => w.field.key)
          )
        );
        return outcome;
      }
      toast(
        `${section.title}: ${plural(writes.length, "setting", "settings")} applied ${expiry.words}`
      );
      await settle(writes.map((w) => w.field.key));
      return { ok: true as const, data: null };
    });

  /** Puts every draft back to what runs */
  export const reset = () => {
    draft = { ...running };
    errors = {};
    moved = [];
    defaulted.clear();
    for (const f of fields) written[f.key] = (written[f.key] ?? 0) + 1;
  };

  /** Whether every change can be saved to a file, which the page's Save all needs */
  export const allSaveable = () => changed.every((f) => edits[f.key].mode === "save");

  const copying = new CopyButton();
  // shown to copy by hand when the browser won't copy it, as over plain http
  let nix = $state<string | null>(null);
  const copyNix = async () => {
    const entries = fields.flatMap((f): [string, unknown][] => {
      const parsed = fromDraft(f, controls[f.key], draft[f.key], view.fields[f.key]);
      if (!("set" in parsed) || parsed.set === REDACTED || parsed.set === null) return [];
      return [[f.key, parsed.set]];
    });
    const text = asNixSettings(entries);
    if (!(await copying.copy(text))) nix = text;
  };
</script>

<Card
  title={section.title}
  id={section.id}
  data-section={section.id}
  class="scroll-mt-gutter"
  hidden={!found.length}>
  {#snippet action()}
    <Button variant="ghost" size="sm" class="text-fg-muted" onclick={copyNix}>
      {#if copying.copied}<CheckIcon />{:else}<CopyIcon />{/if}
      {copying.copied ? "Copied" : "Copy as Nix"}
    </Button>
  {/snippet}
  {#if section.extra}<section.extra />{/if}
  {#if nix}
    <div class="flex flex-col gap-1">
      <p class="text-xs text-fg-muted">
        The browser blocked copying. Copy the text from this box.
        <Button variant="link" class="ml-1" onclick={() => (nix = null)}>Close</Button>
      </p>
      <Textarea class="field-sizing-content max-h-64 font-mono text-xs" readonly value={nix} />
    </div>
  {/if}
  <div class="-mx-5 divide-y border-y">
    {#each fields as field, i (field.key)}
      {#if opensHeading(field, i)}
        <h4 class="border-b-0! px-5 pt-5 text-xs font-semibold text-fg-muted">
          {field.heading}
        </h4>
      {/if}
      <!-- the search hides a row rather than unmounting it, so an edit in progress is kept -->
      <div class={[!found.includes(field) && "hidden"]}>
        <FieldRow
          {field}
          generation={written[field.key] ?? 0}
          info={view.fields[field.key]}
          control={controls[field.key]}
          edit={edits[field.key]}
          values={view.values}
          bind:draft={draft[field.key]}
          changed={changed.includes(field)}
          moved={moved.includes(field.key)}
          unused={!enabled(field)}
          error={errors[field.key]}
          custom={custom(field.key)}
          ondefault={() => toDefault(field.key)}>
          {#snippet extra()}
            {#if field.key === "web.authentication.oidc"}
              <OidcTest names={Object.keys(valueAt(view.values, field.key) ?? {})} />
            {:else if field.key === "soulseek.description" || field.key === "soulseek.picture"}
              <a href={href("/profile")} class="text-xs text-accent hover:underline"
                >Edit on the profile page</a>
            {:else if field.key === "shares.directories"}
              <AddShare explain={false} />
            {:else if field.key === "search.filter_presets"}
              <span class="text-xs text-fg-muted"
                >Presets can also be saved and removed from search results.</span>
            {/if}
          {/snippet}
        </FieldRow>
      </div>
    {/each}
  </div>
  {#if saveable || runtimeable}
    <!-- the section's own actions; the page's bar, pinned to the window, counts every section -->
    <div
      class={[
        "-mx-2 -mb-2 flex flex-wrap items-center gap-inline rounded-card border py-2 pr-2 pl-4 transition-[border-color,background-color] duration-(--motion-fast)",
        changed.length ? "border-accent/40 bg-accent-soft/30" : "border-transparent"
      ]}>
      <span class="text-sm text-fg-muted tabular-nums" aria-live="polite">
        {changed.length ? `${plural(changed.length, "setting", "settings")} changed` : "No changes"}
      </span>
      <div class="ml-auto flex flex-wrap items-center gap-inline">
        {#if changed.length}
          <Button variant="ghost" onclick={reset}>Reset</Button>
        {/if}
        {#if runtimeable}
          <Menu.Root>
            <Menu.Trigger disabled={!changed.length || applying.running}>
              {#snippet child({ props })}
                <Button {...props} variant="outline">
                  Apply at runtime <ChevronDownIcon />
                </Button>
              {/snippet}
            </Menu.Trigger>
            <Menu.Content align="end">
              {#each EXPIRIES as expiry (expiry.label)}
                <Menu.Item onSelect={() => apply(expiry)}>{expiry.label}</Menu.Item>
              {/each}
            </Menu.Content>
          </Menu.Root>
        {/if}
        {#if saveable}
          <Button disabled={!changed.length || saving.running} onclick={save}>Save</Button>
        {/if}
      </div>
    </div>
  {/if}
</Card>
