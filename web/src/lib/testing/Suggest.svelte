<script lang="ts">
  import * as Suggest from "#lib/components/ui/suggest/suggest.js";

  // a suggesting box with three entries, the first of them what Enter takes at rest
  type Props = { value: string; onchoose: (value: string) => void };
  let { value = $bindable(), onchoose }: Props = $props();
</script>

<Suggest.Box bind:value first="one" {onchoose} aria-label="Find">
  <Suggest.Item value="one">One</Suggest.Item>
  <Suggest.Item value="two">Two</Suggest.Item>
  <Suggest.Item value="three" disabled>Three</Suggest.Item>
</Suggest.Box>
