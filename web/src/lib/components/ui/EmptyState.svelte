<script lang="ts">
  import type { Component, Snippet } from "svelte";

  import { cn } from "#lib/ui/cn.js";

  type Props = {
    icon?: Component;
    title: string;
    description?: string;
    action?: Snippet;
    class?: string;
  };

  let { icon: Icon, title, description, action, class: className }: Props = $props();
</script>

<div class={cn("flex flex-col items-center gap-2 px-6 py-10 text-center", className)}>
  {#if Icon}
    <span class="mb-1 flex size-10 items-center justify-center rounded-full bg-surface-muted">
      <Icon class="size-5 text-fg-muted" />
    </span>
  {/if}
  <p class="text-sm font-medium">{title}</p>
  {#if description}
    <p class="max-w-sm text-sm text-pretty text-fg-muted">{description}</p>
  {/if}
  {#if action}<div class="mt-2">{@render action()}</div>{/if}
</div>
