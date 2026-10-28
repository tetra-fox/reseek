<script lang="ts">
  import Badge from "#lib/components/ui/Badge.svelte";

  // a user's country as its two-letter code, its name on hover
  let { code }: { code: string } = $props();

  const names = new Intl.DisplayNames(undefined, { type: "region" });
  const name = $derived.by(() => {
    try {
      return names.of(code.toUpperCase()) ?? code;
    } catch {
      // not a region code the browser knows: the code alone says it
      return code;
    }
  });
</script>

<Badge tone="outline" title={name}>{code.toUpperCase()}</Badge>
