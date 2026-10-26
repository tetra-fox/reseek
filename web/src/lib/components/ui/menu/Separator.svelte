<script lang="ts">
  import { DropdownMenu } from "bits-ui";

  let props: Omit<DropdownMenu.SeparatorProps, "class"> = $props();
</script>

<DropdownMenu.Separator class="-mx-1 my-1 h-px bg-border" {...props} />
