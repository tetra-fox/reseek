<script lang="ts">
  import { page } from "$app/state";

  import JobsPage from "#lib/components/jobs/JobsPage.svelte";
  import { route } from "#lib/nav.js";

  // `?job=<id>` opens that job's parts, as links from the notifications and the schedules do
  const shown = $derived(route(page.url).query.get("job"));
</script>

<JobsPage {shown} />
