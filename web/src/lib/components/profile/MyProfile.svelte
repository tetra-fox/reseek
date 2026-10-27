<script lang="ts">
  import { beforeNavigate } from "$app/navigation";
  import ImageUpIcon from "@lucide/svelte/icons/image-up";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { untrack } from "svelte";

  import { api } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import Avatar from "#lib/components/users/Avatar.svelte";
  import ProfileView from "#lib/components/users/ProfileView.svelte";
  import UserCard from "#lib/components/users/UserCard.svelte";
  import { editability } from "#lib/config/editability.js";
  import { valueAt } from "#lib/config/sections.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { ownProfile } from "#lib/profile.svelte.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";
  import { users } from "#lib/users.svelte.js";

  // our own profile: the text and picture on the left, and on the right how others see them
  const PICTURE_TYPES = ["image/jpeg", "image/png", "image/gif", "image/webp"];
  const MAX_PICTURE = 1024 * 1024;

  const admin = $derived(session.can("Administrator"));
  const view = new Loader(() => call(() => api.GET("/api/v0/options/view")));
  $effect(() => {
    if (admin) untrack(() => void view.load());
  });
  ownProfile.need();

  const profile = $derived(ownProfile.data);
  const name = $derived(app.state?.user.username ?? session.principal?.name ?? "");
  const remote = $derived(valueAt(view.data?.values, "remote_configuration") === true);
  const textEdit = $derived(view.data ? editability(view.data, "soulseek.description") : null);
  const pictureEdit = $derived(view.data ? editability(view.data, "soulseek.picture") : null);
  const canText = $derived(admin && remote && !!textEdit && textEdit.mode !== "locked");
  const canPicture = $derived(admin && remote && !!pictureEdit && pictureEdit.mode !== "locked");

  let text = $state("");
  /** the text as saved, which the draft is compared with */
  let saved = $state<string | null>(null);
  let picked = $state<File | null>(null);
  let pickedUrl = $state<string | null>(null);
  let removed = $state(false);
  let error = $state("");
  let saving = $state(false);
  let input = $state<HTMLInputElement | null>(null);
  let over = $state(false);

  // the saved text arrives, or changes elsewhere while nothing is typed: the draft follows it
  $effect(() => {
    const from = profile?.description;
    if (from === undefined) return;
    untrack(() => {
      if (saved === null || text === saved) text = from;
      saved = from;
    });
  });
  $effect(() => {
    const file = picked;
    if (!file) return;
    const url = URL.createObjectURL(file);
    pickedUrl = url;
    return () => {
      URL.revokeObjectURL(url);
      pickedUrl = null;
    };
  });

  const dirty = $derived((saved !== null && text !== saved) || !!picked || removed);
  const shownPicture = $derived(removed ? null : (profile?.pictureUrl ?? null));

  const take = (file: File | undefined) => {
    if (!file) return;
    if (!PICTURE_TYPES.includes(file.type)) {
      error = "Choose a JPEG, PNG, GIF or WebP picture";
      return;
    }
    if (file.size > MAX_PICTURE) {
      error = "Choose a picture of at most 1 MiB";
      return;
    }
    error = "";
    picked = file;
    removed = false;
  };

  const discard = () => {
    text = saved ?? "";
    picked = null;
    removed = false;
    error = "";
  };

  const save = async () => {
    saving = true;
    error = "";
    const steps: (() => Promise<{ ok: boolean; error?: string }>)[] = [];
    const file = picked;
    if (file) {
      steps.push(() =>
        call(() =>
          api.PUT("/api/v0/profile/picture", {
            body: file as never,
            headers: { "Content-Type": file.type },
            bodySerializer: (body) => body as BodyInit
          })
        )
      );
    } else if (removed && profile?.hasPicture) {
      steps.push(() => call(() => api.DELETE("/api/v0/profile/picture")));
    }
    if (saved !== null && text !== saved) {
      const description = text;
      steps.push(() => call(() => api.PUT("/api/v0/profile", { body: { description } })));
    }
    for (const step of steps) {
      const outcome = await step();
      if (!outcome.ok) {
        error = outcome.error ?? "Not saved";
        break;
      }
    }
    saving = false;
    await ownProfile.loader.load();
    if (error) return;
    saved = text;
    picked = null;
    removed = false;
    toast("Profile saved");
  };

  beforeNavigate(({ cancel, type }) => {
    if (dirty && type !== "leave" && !confirm("Leave without saving your profile?")) cancel();
  });
  $effect(() => {
    if (!dirty) return;
    const warn = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  });

  const me = $derived(users.get(name));
  $effect(() => {
    if (name) void users.load(name);
  });
</script>

{#if !admin}
  <Notice kind="info">Only administrators change the profile; this is how others see it.</Notice>
{:else if view.data && !remote}
  <Notice kind="info">
    Changes from here need <code>remote_configuration</code> on; until then the profile is only shown.
  </Notice>
{/if}
<LoadError loader={ownProfile.loader} />

{#if !profile}
  <Skeleton class="h-80" />
{:else}
  <div class="grid gap-section md:grid-cols-2">
    <Card title="Edit">
      <div class="flex items-center gap-4">
        <!-- a picture dropped here is taken like one picked -->
        <div
          role="region"
          aria-label="Picture, drop one here"
          class={["rounded-full", over && "ring-2 ring-accent"]}
          ondragover={(e) => {
            if (!canPicture) return;
            e.preventDefault();
            over = true;
          }}
          ondragleave={() => (over = false)}
          ondrop={(e) => {
            if (!canPicture) return;
            e.preventDefault();
            over = false;
            take(e.dataTransfer?.files[0]);
          }}>
          <Avatar {name} pictureUrl={shownPicture} src={pickedUrl} class="size-32 text-4xl" />
        </div>
        <div class="flex flex-col gap-2">
          <input
            bind:this={input}
            type="file"
            accept={PICTURE_TYPES.join(",")}
            class="hidden"
            aria-label="Picture file"
            onchange={(e) => {
              take(e.currentTarget.files?.[0]);
              e.currentTarget.value = "";
            }} />
          <Button variant="outline" size="sm" disabled={!canPicture} onclick={() => input?.click()}>
            <ImageUpIcon />Upload
          </Button>
          <Button
            variant="ghost"
            size="sm"
            disabled={!canPicture || (!profile.hasPicture && !picked) || removed}
            onclick={() => {
              picked = null;
              removed = profile.hasPicture;
            }}>
            <Trash2Icon />Remove
          </Button>
        </div>
      </div>
      {#if pictureEdit?.notice}<p class="text-xs text-fg-muted">{pictureEdit.notice}</p>{/if}
      <div class="flex flex-col gap-1">
        <label for="profile-text" class="text-xs font-medium text-fg-muted">Profile text</label>
        <Textarea
          id="profile-text"
          class="min-h-48 text-sm"
          placeholder="What other users read about you"
          bind:value={text}
          disabled={!canText} />
        <p class="flex justify-between text-xs text-fg-muted">
          <span>{textEdit?.notice ?? ""}</span>
          <span class="tabular-nums">{[...text].length} characters</span>
        </p>
      </div>
      {#if error}<Notice kind="error">{error}</Notice>{/if}
      <div class="flex justify-end gap-inline">
        <Button variant="ghost" disabled={!dirty || saving} onclick={discard}>Discard</Button>
        <Button disabled={!dirty || saving || (!canText && !canPicture)} onclick={() => void save()}
          >Save</Button>
      </div>
    </Card>

    <Card title="How others see you">
      <ProfileView
        {name}
        pictureUrl={shownPicture}
        src={pickedUrl}
        description={text}
        uploadSlots={profile.uploadSlots}
        freeUploadSlot={profile.freeUploadSlot}
        queueLength={profile.queueLength} />
      <div class="mt-2 w-80 max-w-full rounded-card border bg-overlay p-3 shadow-raised">
        <UserCard
          {name}
          user={me}
          onblock={() => {}}
          draft={{ description: text, src: pickedUrl, pictureUrl: shownPicture }} />
      </div>
    </Card>
  </div>
{/if}
