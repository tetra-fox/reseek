<script lang="ts">
  import SearchIcon from "@lucide/svelte/icons/search";

  import * as Suggest from "#lib/components/ui/suggest/suggest.js";
  import { UserFinder } from "#lib/users/finder.svelte.js";

  import UserEntries from "./UserEntries.svelte";

  type Props = {
    /** the text a moment after typing stops, which the list under the box filters by */
    onsearch: (text: string) => void;
    /** what is typed in the box */
    typed?: string;
  };

  let { onsearch, typed = $bindable("") }: Props = $props();

  let open = $state(false);

  const text = $derived(typed.trim());
  const finder = new UserFinder(() => text, {
    onread: (value) => onsearch(value),
    onmissed: () => (open = true)
  });
</script>

<Suggest.Box
  bind:value={typed}
  bind:open
  first={finder.first}
  listed={finder.unknown || !!finder.matches.length}
  hold={finder.asking}
  settle={finder.settled}
  onchoose={finder.choose}
  ondismiss={finder.abandon}
  icon={SearchIcon}
  wrapperClass="w-full max-w-xs"
  type="search"
  autocomplete="off"
  placeholder="Username, tag or note"
  aria-label="Search users">
  <UserEntries {finder} {text} />
</Suggest.Box>
