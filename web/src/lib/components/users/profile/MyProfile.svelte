<script lang="ts">
  import { beforeNavigate } from "$app/navigation";
  import { untrack } from "svelte";

  import { api } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import UserCard from "#lib/components/users/UserCard.svelte";
  import { editability } from "#lib/config/editability.js";
  import { valueAt } from "#lib/config/sections.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { plural } from "#lib/format.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { OwnInterests } from "#lib/users/interests.svelte.js";
  import { ownProfile } from "#lib/users/profile.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

  import AccountCard from "./AccountCard.svelte";
  import InterestsEditor from "./InterestsEditor.svelte";
  import PicturePicker from "./PicturePicker.svelte";
  import ProfileView from "./ProfileView.svelte";

  // everything about our own profile: what others are sent and the account behind it, with how
  // others see it always in view, beside the editors or above them on a phone

  const admin = $derived(session.can("Administrator"));
  const view = new Loader(() => call(() => api.GET("/api/v0/options/view")));
  $effect(() => {
    if (admin) untrack(() => void view.load());
  });
  ownProfile.need();
  const interests = new OwnInterests();
  void interests.loader.load();

  const profile = $derived(ownProfile.data);
  const name = $derived(app.state?.user.username ?? session.principal?.name ?? "");
  const remote = $derived(valueAt(view.data?.values, "remote_configuration") === true);
  const textEdit = $derived(
    view.data ? editability(view.data, "soulseek.description", "page") : null
  );
  const pictureEdit = $derived(
    view.data ? editability(view.data, "soulseek.picture", "page") : null
  );
  const canText = $derived(admin && remote && !!textEdit && textEdit.mode !== "locked");
  const canPicture = $derived(admin && remote && !!pictureEdit && pictureEdit.mode !== "locked");
  const passwordEdit = $derived(
    view.data ? editability(view.data, "soulseek.password", "page") : null
  );
  const canPassword = $derived(admin && remote && !!passwordEdit && passwordEdit.mode !== "locked");
  // the server takes a new password at once, so the one in the configuration stops working
  const passwordNotice = $derived(
    passwordEdit?.mode === "runtime"
      ? `${passwordEdit.notice} Add the new password to the configuration as well.`
      : (passwordEdit?.notice ?? null)
  );

  let text = $state("");
  /** the text as saved, which the draft is compared with */
  let saved = $state<string | null>(null);
  let picked = $state<File | null>(null);
  let pickedUrl = $state<string | null>(null);
  let removed = $state(false);
  let error = $state("");
  let saving = $state(false);

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

  const shownLists = $derived([
    { title: "Interests", list: interests.lists.liked },
    { title: "Dislikes", list: interests.lists.hated }
  ]);
  const dirty = $derived((saved !== null && text !== saved) || !!picked || removed);
  const shownPicture = $derived(removed ? null : (profile?.pictureUrl ?? null));

  const pick = (file: File) => {
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

  /**
   * Saves the draft as it is now; what changes while it runs stays a change to save, and the
   * save is over once the profile has been read again
   */
  const save = async () => {
    saving = true;
    error = "";
    const steps: (() => Promise<{ ok: boolean; error?: string }>)[] = [];
    const file = picked;
    const removing = removed;
    const description = text;
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
    if (saved !== null && description !== saved) {
      steps.push(() => call(() => api.PUT("/api/v0/profile", { body: { description } })));
    }
    for (const step of steps) {
      const outcome = await step();
      if (!outcome.ok) {
        error = outcome.error ?? "Could not save the profile";
        break;
      }
    }
    await ownProfile.loader.load();
    saving = false;
    if (error) return;
    saved = description;
    if (picked === file) picked = null;
    if (removing) removed = false;
    toast("Profile saved");
  };

  beforeNavigate(({ cancel, type }) => {
    if (dirty && type !== "leave" && !confirm("Leave without saving the profile?")) cancel();
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
  <Notice kind="info">Only administrators can change the profile.</Notice>
{:else if view.data && !remote}
  <Notice kind="info">
    Editing the profile requires <code>remote_configuration</code>. Until it is enabled, the profile
    is read-only.
  </Notice>
{/if}
<LoadError loader={ownProfile.loader} />

{#if !profile}
  <Skeleton class="h-80" />
{:else}
  <!-- a column never takes its content's width, so large text wraps rather than widens it. The
       public view comes first, for a phone; beside the editors it goes to the right -->
  <div class="grid grid-cols-[minmax(0,1fr)] items-start gap-section md:grid-cols-2">
    <div class="md:sticky md:top-gutter md:col-start-2 md:row-start-1">
      <Card title="What others see">
        <ProfileView
          {name}
          pictureUrl={shownPicture}
          src={pickedUrl}
          description={text}
          uploadSlots={profile.uploadSlots}
          freeUploadSlot={profile.freeUploadSlot}
          queueLength={profile.queueLength} />
        {#if shownLists.some(({ list }) => list.length)}
          <dl class="flex flex-col gap-1 text-xs">
            {#each shownLists as { title, list } (title)}
              {#if list.length}
                <div class="flex gap-inline">
                  <dt class="shrink-0 text-fg-muted">{title}</dt>
                  <dd class="min-w-0 wrap-anywhere">{list.join(", ")}</dd>
                </div>
              {/if}
            {/each}
          </dl>
        {/if}
        <div class="w-80 max-w-full rounded-card border bg-overlay p-3 shadow-raised">
          <UserCard
            {name}
            user={me}
            onblock={() => {}}
            draft={{ description: text, src: pickedUrl, pictureUrl: shownPicture }} />
        </div>
      </Card>
    </div>

    <div class="flex min-w-0 flex-col gap-section md:col-start-1 md:row-start-1">
      <Card title="Picture and text">
        <PicturePicker
          {name}
          pictureUrl={shownPicture}
          src={pickedUrl}
          disabled={!canPicture}
          removable={!removed && (profile.hasPicture || !!picked)}
          onpick={pick}
          onrefuse={(reason) => (error = reason)}
          onremove={() => {
            picked = null;
            removed = profile.hasPicture;
          }} />
        {#if pictureEdit?.notice}<p class="text-xs text-fg-muted">{pictureEdit.notice}</p>{/if}
        <div class="flex flex-col gap-1">
          <label for="profile-text" class="text-xs font-medium text-fg-muted">Profile text</label>
          <Textarea
            id="profile-text"
            class="min-h-48 text-sm"
            placeholder="Text shown to other users"
            bind:value={text}
            disabled={!canText} />
          <p class="flex justify-between gap-inline text-xs text-fg-muted">
            <span>{textEdit?.notice ?? ""}</span>
            <span class="shrink-0 tabular-nums"
              >{plural([...text].length, "character", "characters")}</span>
          </p>
        </div>
        {#if error}<Notice kind="error">{error}</Notice>{/if}
        <div class="flex justify-end gap-inline">
          <Button variant="ghost" disabled={!dirty || saving} onclick={discard}>Discard</Button>
          <Button
            disabled={!dirty || saving || (!canText && !canPicture)}
            onclick={() => void save()}>Save</Button>
        </div>
      </Card>

      <Card
        title="Interests and dislikes"
        description="Told to the Soulseek server, which matches you with people who share them. Saved as you go.">
        <InterestsEditor {interests} disabled={!admin} />
      </Card>

      <AccountCard {name} allowed={canPassword} notice={passwordNotice} />
    </div>
  </div>
{/if}
