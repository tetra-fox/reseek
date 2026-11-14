<script lang="ts">
  import LockIcon from "@lucide/svelte/icons/lock";

  import { app } from "#lib/app.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";

  const signOut = async () => {
    const error = await app.signOut();
    if (error) toast(`Could not log out: ${error}`, "error");
  };
</script>

<!-- the session is kept and checked again every few seconds, so the app comes back by itself -->
<div class="flex flex-col items-center gap-3 px-6 py-10 text-center" role="alert">
  <span class="flex size-10 items-center justify-center rounded-full bg-warning-soft">
    <LockIcon class="size-5 text-warning-soft-fg" />
  </span>
  <p class="text-sm font-medium">No access right now</p>
  <p class="max-w-sm text-sm text-pretty text-fg-muted">
    This page comes back by itself once your access returns
  </p>
  <div class="flex gap-inline">
    <Button variant="outline" onclick={() => void app.bootstrap()}>Check now</Button>
    <Button variant="ghost" onclick={() => void signOut()}>Sign out</Button>
  </div>
</div>
