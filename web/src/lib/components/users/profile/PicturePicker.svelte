<script lang="ts">
  import ImageUpIcon from "@lucide/svelte/icons/image-up";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";

  import Button from "#lib/components/ui/Button.svelte";
  import Avatar from "#lib/components/users/Avatar.svelte";

  // the picture to send: dropped anywhere on the box or chosen, the limits stated before either.
  // Nothing goes to the daemon until the profile is saved
  type Props = {
    name: string;
    pictureUrl: string | null;
    /** a picture picked and not saved yet */
    src: string | null;
    /** what the picture shows */
    alt: string;
    disabled: boolean;
    /** there is a picture to take away */
    removable: boolean;
    /** a file within the limits was picked */
    onpick: (file: File) => void;
    /** a file was refused, and why */
    onrefuse: (reason: string) => void;
    onremove: () => void;
  };

  let { name, pictureUrl, src, alt, disabled, removable, onpick, onrefuse, onremove }: Props =
    $props();

  const TYPES = ["image/jpeg", "image/png", "image/gif", "image/webp", "image/bmp"];
  const FORMATS = "JPEG, PNG, GIF, WebP or BMP";
  const MAX_BYTES = 1024 * 1024;
  const MAX_SIZE = "1 MiB";

  let input = $state<HTMLInputElement | null>(null);
  let over = $state(false);

  const take = (file: File | undefined) => {
    if (!file) return;
    if (!TYPES.includes(file.type)) onrefuse(`Choose a ${FORMATS} picture`);
    else if (file.size > MAX_BYTES) onrefuse(`Choose a picture no larger than ${MAX_SIZE}`);
    else onpick(file);
  };
</script>

<!-- a drag moving across the box's children stays over it, so only leaving the box ends it -->
<div
  role="region"
  aria-label="Picture"
  class={[
    "flex flex-wrap items-center gap-4 rounded-control border border-dashed p-4 transition-colors duration-(--motion-fast)",
    over && "border-accent bg-accent/10"
  ]}
  ondragover={(e) => {
    if (disabled) return;
    e.preventDefault();
    over = true;
  }}
  ondragleave={(e) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) over = false;
  }}
  ondrop={(e) => {
    if (disabled) return;
    e.preventDefault();
    over = false;
    take(e.dataTransfer?.files[0]);
  }}>
  <Avatar {name} {pictureUrl} {src} {alt} link={false} class="size-24 text-3xl" />
  <div class="flex min-w-48 flex-1 flex-col gap-2">
    <p class="text-sm">{over ? "Drop to use this picture" : "Drag a picture here"}</p>
    <p class="text-xs text-fg-muted">
      {FORMATS} up to {MAX_SIZE}, scaled down to fit 512 pixels
    </p>
    <input
      bind:this={input}
      type="file"
      accept={TYPES.join(",")}
      class="hidden"
      aria-label="Picture file"
      onchange={(e) => {
        take(e.currentTarget.files?.[0]);
        e.currentTarget.value = "";
      }} />
    <div class="flex flex-wrap gap-inline">
      <Button variant="outline" size="sm" {disabled} onclick={() => input?.click()}>
        <ImageUpIcon />Choose picture
      </Button>
      {#if removable}
        <Button variant="ghost" size="sm" {disabled} onclick={onremove}>
          <Trash2Icon />Remove
        </Button>
      {/if}
    </div>
  </div>
</div>
