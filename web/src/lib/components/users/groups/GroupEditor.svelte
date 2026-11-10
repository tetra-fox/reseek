<script lang="ts">
  import { untrack } from "svelte";

  import { api } from "#lib/api/client.js";
  import Field from "#lib/components/Field.svelte";
  import Form from "#lib/components/Form.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { clearing } from "#lib/config/editability.js";
  import type { ConfigView } from "#lib/config/fields.js";
  import { valueAt } from "#lib/config/sections.js";
  import { type Views, writeFromPage } from "#lib/config/write.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { call } from "#lib/request.js";
  import { audienceText } from "#lib/system/shares.js";
  import { GROUP_COLORS, groupColor } from "#lib/users/colors.js";
  import {
    alwaysSet,
    audienceOf,
    BUDDIES,
    DEFAULT,
    groupPatch,
    type GroupSummary,
    LEECHERS,
    nameProblem,
    notSetBy,
    ORDER,
    settingNames,
    STRATEGIES,
    type Upload,
    UPLOAD_COUNTS,
    uploadDraft,
    uploadOf,
    USER_DEFINED,
    VISIBLE_TO,
    type VisibleTo,
    withHidden
  } from "#lib/users/groups.js";

  import CountInput from "./CountInput.svelte";

  // one group's settings as a form: a user-defined group's name and color, the upload settings,
  // the leechers' thresholds and the shares the group sees. Each upload setting a group leaves
  // not set comes from a lower group, and for everyone from the global transfer settings
  type Props = {
    /** the group, null for a new one */
    group: GroupSummary | null;
    view: ConfigView;
    /** where `view` comes from, read again once a save is written */
    views: Views;
    /** the aliases of the shares there are, and of those the options name */
    aliases: string[];
    /** the names of the groups there are, which a new name can't take */
    taken: string[];
    onclose: () => void;
    onsaved: () => void;
  };

  let { group, view, views, aliases, taken, onclose, onsaved }: Props = $props();

  // the form starts from the options as they were when it opened
  const start = untrack(() => {
    const name = group?.name ?? "";
    const own = !group || !group.builtin;
    const upload = (
      own
        ? valueAt(view.values, `${USER_DEFINED}.${name}.upload`)
        : valueAt(view.values, `transfers.groups.${name}.upload`)
    ) as Upload | null | undefined;
    const visibleTo = (valueAt(view.values, VISIBLE_TO) ?? {}) as VisibleTo;
    const thresholds = (valueAt(view.values, "transfers.groups.leechers.thresholds") ?? {}) as {
      files?: number;
      directories?: number;
    };
    return {
      name,
      own,
      upload: upload ?? null,
      draft: uploadDraft(upload),
      color: String(valueAt(view.values, `${USER_DEFINED}.${name}.color`) ?? GROUP_COLORS[0]),
      visibleTo,
      seen: aliases.filter((a) => audienceOf(visibleTo, a)?.includes(name)),
      files: String(thresholds.files ?? ""),
      directories: String(thresholds.directories ?? "")
    };
  });

  /** a user-defined group or a new one, whose name and color are ours */
  const own = start.own;
  /** everyone's settings, whose not set is the global transfer settings */
  const everyone = start.name === DEFAULT;
  /** the group can be shown shares only some groups see */
  const sees = own || start.name === BUDDIES;

  let name = $state(start.name);
  let color = $state(start.color);
  let draft = $state({ ...start.draft });
  let seen = $state([...start.seen]);
  let files = $state(start.files);
  let directories = $state(start.directories);
  let error = $state("");
  let saving = $state(false);

  const problem = $derived(
    own && (!start.name || name !== start.name)
      ? nameProblem(
          name,
          taken.filter((t) => t !== start.name)
        )
      : null
  );
  /**
   * why a built-in group's setting can't be cleared here, for one a file holds; a user-defined
   * group is written whole, which clears what it leaves out
   */
  const held = (key: string) =>
    own
      ? null
      : clearing(
          view,
          key in start.draft
            ? `transfers.groups.${start.name}.upload.${key}`
            : `transfers.groups.leechers.thresholds.${key}`
        );
  /** the label of a setting the form has emptied though a file holds it */
  const emptiedHeld = () =>
    [
      ...UPLOAD_COUNTS.map((f) => ({ key: f.key, label: f.label, text: draft[f.key] })),
      { key: "strategy", label: "Queue order", text: draft.strategy },
      ...(start.name === LEECHERS
        ? [
            { key: "files", label: "Files", text: files },
            { key: "directories", label: "Folders", text: directories }
          ]
        : [])
    ].find((f) => !f.text.trim() && held(f.key));

  /** what an empty field means: a lower group's, or for everyone the global setting */
  const unset = (key: string) => {
    if (!everyone) return "Not set";
    const global = valueAt(view.values, `transfers.upload.${key}`);
    if (typeof global === "number" && global > 0) return `Global setting (${global})`;
    return `Global setting (${key === "slots" ? "every slot" : "no limit"})`;
  };

  /** the leechers' settings as drafted, for what they give buddies who share too little */
  const leeching = $derived.by(() => {
    if (start.name !== LEECHERS) return null;
    const parsed = uploadOf(draft);
    const names = "upload" in parsed ? settingNames(parsed.upload) : [];
    if (!names.length) return null;
    const order = (valueAt(view.values, ORDER) ?? []) as string[];
    if (order.indexOf(BUDDIES) > order.indexOf(LEECHERS))
      return "Buddies who share too little get all of these";
    const still = notSetBy(
      names,
      valueAt(view.values, `transfers.groups.${BUDDIES}.upload`) as Upload | null
    );
    return still.length ? `Buddies who share too little still get: ${still.join(", ")}` : null;
  });

  const save = async () => {
    if (problem) return void (error = problem);
    const emptied = emptiedHeld();
    if (emptied) return void (error = `${emptied.label}: ${held(emptied.key)}`);
    const change = groupPatch(view.values, {
      from: start.name,
      own,
      name,
      color,
      draft,
      started: start.draft,
      seen: sees ? seen : null,
      thresholds:
        start.name === LEECHERS
          ? { files, directories, was: { files: start.files, directories: start.directories } }
          : undefined
    });
    if ("error" in change) return void (error = change.error);
    if (!Object.keys(change.set).length && !change.unset.length) return onclose();
    saving = true;
    const outcome = await writeFromPage(views, change.set, change.unset);
    if (!outcome.ok) {
      saving = false;
      error = outcome.error;
      return;
    }
    // members stay in a group by its name, so they follow a renamed group
    if (start.name && name !== start.name && group?.members) {
      const moved = await call(() =>
        api.POST("/api/v0/groups/{name}/members/move", {
          params: { path: { name: start.name } },
          body: { to: name }
        })
      );
      if (!moved.ok) toast(`Could not move the members of ${start.name}: ${moved.error}`, "error");
    }
    saving = false;
    toast(withHidden(start.name ? `Group ${name} saved` : `Group ${name} created`, change.hidden));
    onsaved();
    onclose();
  };

  const toggleShare = (alias: string, on: boolean) => {
    seen = on ? [...seen, alias] : seen.filter((a) => a !== alias);
  };
</script>

<Dialog
  bind:open={
    () => true,
    (o) => {
      if (!o && !saving) onclose();
    }
  }
  busy={saving}
  title={start.name ? `Group ${start.name}` : "New group"}
  class="w-[min(40rem,calc(100vw-2rem))]">
  <Form
    layout="stacked"
    id="group-form"
    class="flex flex-col gap-group"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}>
    {#if own}
      <div class="grid grid-cols-[minmax(0,1fr)] gap-group sm:grid-cols-2">
        <Field label="Name" for="group-name" hint={(name && problem) || undefined}>
          <!-- names are lower case words, so what is typed is written as one -->
          <Input
            id="group-name"
            class="font-mono"
            maxlength={32}
            autocomplete="off"
            spellcheck="false"
            aria-invalid={problem ? true : undefined}
            value={name}
            oninput={(e) => (name = e.currentTarget.value.toLowerCase().replace(/\s+/g, "-"))} />
        </Field>
        <Field label="Color">
          <div role="radiogroup" aria-label="Color" class="flex flex-wrap gap-1.5">
            {#each GROUP_COLORS as c (c)}
              <Tooltip text={c} dense>
                {#snippet child({ props })}
                  <button
                    {...props}
                    type="button"
                    role="radio"
                    aria-checked={color === c}
                    aria-label={c}
                    class={[
                      "size-6 rounded-full border-2 outline-offset-2",
                      color === c ? "border-fg" : "border-transparent"
                    ]}
                    style:background-color={groupColor(c)}
                    onclick={() => (color = c)}></button>
                {/snippet}
              </Tooltip>
            {/each}
          </div>
        </Field>
      </div>
    {/if}

    <section class="flex flex-col gap-3">
      <h4 class="text-sm font-medium">Upload settings</h4>
      <div class="grid grid-cols-[minmax(0,1fr)] gap-3 sm:grid-cols-2">
        <Field label="Queue order" for="group-strategy" hint={held("strategy") ?? undefined}>
          <Select
            id="group-strategy"
            bind:value={draft.strategy}
            options={[
              ...(held("strategy") ? [] : [{ value: "", label: "Not set" }]),
              ...Object.entries(STRATEGIES).map(([value, label]) => ({ value, label }))
            ]} />
        </Field>
        {#each UPLOAD_COUNTS.filter((f) => !f.limit) as field (field.key)}
          <Field label={field.label} for={`group-${field.key}`} hint={held(field.key) ?? undefined}>
            <CountInput
              id={`group-${field.key}`}
              unset={unset(field.key)}
              none={field.key === "slots" ? "Every slot" : "No limit"}
              always={alwaysSet(start.name, field.key)}
              held={held(field.key)}
              bind:value={draft[field.key]} />
          </Field>
        {/each}
      </div>
      <details class="group/limits" open={UPLOAD_COUNTS.some((f) => f.limit && start.draft[f.key])}>
        <summary class="cursor-pointer text-sm text-fg-muted hover:text-fg"
          >Per-user limits</summary>
        <div class="mt-3 grid grid-cols-[minmax(0,1fr)] gap-3 sm:grid-cols-2">
          {#each UPLOAD_COUNTS.filter((f) => f.limit) as field (field.key)}
            <Field
              label={field.label}
              for={`group-${field.key}`}
              hint={held(field.key) ?? undefined}>
              <CountInput
                id={`group-${field.key}`}
                unset={unset(field.key)}
                none="No limit"
                held={held(field.key)}
                bind:value={draft[field.key]} />
            </Field>
          {/each}
        </div>
      </details>
      {#if leeching}<Notice kind="warning">{leeching}</Notice>{/if}
    </section>

    {#if start.name === LEECHERS}
      <section class="flex flex-col gap-3">
        <h4 class="text-sm font-medium">Leechers share fewer than</h4>
        <div class="grid grid-cols-[minmax(0,1fr)] gap-3 sm:grid-cols-2">
          <Field label="Files" for="group-files" hint={held("files") ?? undefined}>
            <Input
              id="group-files"
              class="tabular-nums"
              inputmode="numeric"
              aria-describedby={held("files") ? "group-files-hint" : undefined}
              bind:value={files} />
          </Field>
          <Field label="Folders" for="group-directories" hint={held("directories") ?? undefined}>
            <Input
              id="group-directories"
              class="tabular-nums"
              inputmode="numeric"
              aria-describedby={held("directories") ? "group-directories-hint" : undefined}
              bind:value={directories} />
          </Field>
        </div>
      </section>
    {/if}

    {#if sees}
      <section class="flex flex-col gap-2">
        <h4 class="text-sm font-medium">Shares</h4>
        {#each aliases as alias (alias)}
          {@const audience = audienceOf(start.visibleTo, alias)}
          <label class="flex items-center gap-2 text-sm">
            <Checkbox
              checked={seen.includes(alias)}
              onCheckedChange={(on) => toggleShare(alias, on)} />
            <span class="min-w-0 truncate font-mono">{alias}</span>
            <span class="ml-auto shrink-0 text-xs text-fg-muted">{audienceText(audience)}</span>
          </label>
        {:else}
          <p class="text-sm text-fg-muted">No shares</p>
        {/each}
        {#if aliases.length}
          <p class="text-xs text-fg-subtle">
            Ticking a share hides it from everyone outside its groups
          </p>
        {/if}
      </section>
    {/if}
  </Form>
  {#if error}<Notice kind="error">{error}</Notice>{/if}
  {#snippet footer()}
    <Button variant="ghost" disabled={saving} onclick={onclose}>Cancel</Button>
    <Button type="submit" form="group-form" disabled={saving || !!problem}
      >{start.name ? "Save" : "Create"}</Button>
  {/snippet}
</Dialog>
