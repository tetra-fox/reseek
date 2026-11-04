<script lang="ts">
  import * as Suggest from "#lib/components/ui/suggest/suggest.js";

  // a suggesting box with three entries, the first of them what Enter takes at rest, once
  // `settle` has answered when it is given
  type Props = {
    value: string;
    onchoose: (value: string) => void;
    settle?: () => Promise<unknown> | undefined;
  };
  let { value = $bindable(), onchoose, settle }: Props = $props();
</script>

<Suggest.Box bind:value first="one" {settle} {onchoose} aria-label="Find">
  <Suggest.Item value="one">One</Suggest.Item>
  <Suggest.Item value="two">Two</Suggest.Item>
  <Suggest.Item value="three" disabled>Three</Suggest.Item>
</Suggest.Box>
