<script lang="ts">
  import { page } from "$app/state";

  import { slots } from "#lib/analysis/slots.svelte.js";
  import AnalysisPage from "#lib/components/analysis/AnalysisPage.svelte";
  import { route } from "#lib/nav.js";

  // the address says what the slots hold, so a link, a reload and Back all show the same
  $effect(() => slots.follow(route(page.url).query));
</script>

<AnalysisPage />
