<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import { untrack } from "svelte";

  import { api } from "#lib/api/client.js";
  import { Busy } from "#lib/busy.svelte.js";
  import { CopyButton } from "#lib/clipboard.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import { editability } from "#lib/config/editability.js";
  import {
    type ConfigView,
    controlOf,
    type Draft,
    errorsByKey,
    fromDraft,
    REDACTED,
    toDraft
  } from "#lib/config/fields.js";
  import { asNixSettings } from "#lib/config/nix.js";
  import { type Field, type Section, valueAt } from "#lib/config/sections.js";
  import { plural } from "#lib/format.js";
  import { same } from "#lib/keyed.svelte.js";
  import { call } from "#lib/request.js";
  import { toast } from "#lib/toast.svelte.js";

  import FieldRow from "./FieldRow.svelte";

  type Props = {
    section: Section;
    view: ConfigView;
    /** reloads the view after a write, or after a write was refused because the file changed */
    onchanged: () => Promise<void>;
  };

  let { section, view, onchanged }: Props = $props();

  const fields = $derived(section.fields.filter((f) => f.key in view.fields));
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
      const parsed = fromDraft(
        field,
        controls[field.key],
        draft[field.key],
        view.fields[field.key].kind
      );
      if ("error" in parsed) found[field.key] = parsed.error;
      else out.push({ field, ...parsed });
    }
    errors = found;
    return Object.keys(found).length ? null : out;
  };

  const saving = new Busy();
  const save = () =>
    saving.run(async () => {
      const writes = collect("save");
      if (!writes) return { ok: true as const, data: null };
      const byFile = new Map<string, typeof writes>();
      for (const write of writes) {
        const file = view.fields[write.field.key].file ?? view.files[0]?.name ?? "config.toml";
        byFile.set(file, [...(byFile.get(file) ?? []), write]);
      }
      let saved = 0;
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
          if (outcome.status === 409 && outcome.error.startsWith("config file changed on disk")) {
            await onchanged();
            return {
              ...outcome,
              error: `${file} changed on disk, check the values and save again`
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
        saved += list.length;
        restart = [...restart, ...outcome.data.restartRequired];
      }
      toast(
        `${section.title}: ${plural(saved, "setting", "settings")} saved` +
          (restart.length ? `, restart reseek for ${restart.join(", ")}` : "")
      );
      await onchanged();
      return { ok: true as const, data: null };
    });

  /** How long a runtime change lasts: an expiry for `expiresIn`, null for until removed */
  const EXPIRIES = [
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
      await onchanged();
      return { ok: true as const, data: null };
    });

  const reset = () => {
    draft = { ...running };
    errors = {};
    moved = [];
  };

  const copying = new CopyButton();
  // shown to copy by hand when the browser won't copy it, as over plain http
  let nix = $state<string | null>(null);
  const copyNix = async () => {
    const entries = fields.flatMap((f): [string, unknown][] => {
      const parsed = fromDraft(f, controls[f.key], draft[f.key], view.fields[f.key].kind);
      if (!("set" in parsed) || parsed.set === REDACTED || parsed.set === null) return [];
      return [[f.key, parsed.set]];
    });
    const text = asNixSettings(entries);
    if (!(await copying.copy(text))) nix = text;
  };
</script>

<Card title={section.title} id={section.id} class="scroll-mt-20">
  {#snippet action()}
    <Button variant="ghost" size="sm" class="text-fg-muted" onclick={copyNix}>
      {#if copying.copied}<CheckIcon />{:else}<CopyIcon />{/if}
      {copying.copied ? "Copied" : "Copy as Nix"}
    </Button>
  {/snippet}
  {#if nix}
    <div class="flex flex-col gap-1">
      <p class="text-xs text-fg-muted">
        The browser won't copy here, copy it from this box
        <Button variant="link" class="ml-1" onclick={() => (nix = null)}>close</Button>
      </p>
      <Textarea class="field-sizing-content max-h-64 font-mono text-xs" readonly value={nix} />
    </div>
  {/if}
  <div class="-mx-5 divide-y border-y">
    {#each fields as field (field.key)}
      <FieldRow
        {field}
        info={view.fields[field.key]}
        control={controls[field.key]}
        edit={edits[field.key]}
        values={view.values}
        bind:draft={draft[field.key]}
        changed={changed.includes(field)}
        moved={moved.includes(field.key)}
        unused={!enabled(field)}
        error={errors[field.key]} />
    {/each}
  </div>
  {#if saveable || runtimeable}
    <!-- pinned to the bottom of the window while the section has unsaved changes -->
    <div
      class={[
        "-mx-2 -mb-2 flex flex-wrap items-center gap-inline rounded-card border py-2 pr-2 pl-4 transition-[border-color,background-color,box-shadow] duration-(--motion-fast)",
        changed.length
          ? "sticky bottom-12 z-10 border-accent/40 bg-surface shadow-raised"
          : "border-transparent"
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
