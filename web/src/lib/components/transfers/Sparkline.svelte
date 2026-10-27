<script lang="ts">
  // the last speeds a transfer reported, as a small line; nothing until there are two
  let { points }: { points: number[] } = $props();

  const W = 48;
  const H = 14;
  const path = $derived.by(() => {
    if (points.length < 2) return "";
    const top = Math.max(...points) || 1;
    return points
      .map(
        (p, i) => `${((i / (points.length - 1)) * W).toFixed(1)},${(H - (p / top) * H).toFixed(1)}`
      )
      .join(" ");
  });
</script>

{#if path}
  <svg
    width={W}
    height={H}
    viewBox={`0 0 ${W} ${H}`}
    class="shrink-0 text-accent"
    aria-hidden="true">
    <polyline points={path} fill="none" stroke="currentColor" stroke-width="1.25" />
  </svg>
{/if}
