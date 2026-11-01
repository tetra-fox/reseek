<script lang="ts">
  import { page } from "$app/state";

  import { slots } from "#lib/analysis/slots.svelte.js";
  import AnalysisPage from "#lib/components/analysis/AnalysisPage.svelte";
  import { route } from "#lib/nav.js";

  // the address says what the slots hold, so a link, a reload and Back all show the same. The URL
  // changes before this page goes, and an address of another screen says nothing about the slots
  $effect(() => {
    const { path, query } = route(page.url);
    if (path === "/analysis") slots.follow(query);
  });
</script>

<AnalysisPage />
