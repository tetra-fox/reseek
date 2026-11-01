<script lang="ts">
  import type { Report } from "#lib/analysis/analysis.js";
  import { VERDICTS } from "#lib/analysis/analysis.js";
  import { formatHz } from "#lib/analysis/spectrogram/frequency.js";
  import { formatDuration } from "#lib/format.js";

  // two reports' main figures side by side, a row whose values differ marked. It states numbers
  // and picks neither file
  type Props = { a: Report; b: Report };

  let { a, b }: Props = $props();

  /** the stream's bitrate for a lossy file, for a lossless one the file's size over its length */
  const kbps = (r: Report) =>
    r.claimed.bitrateKbps ??
    (r.durationSeconds > 0 ? Math.round((r.size * 8) / r.durationSeconds / 1000) : null);
  const number = (v: number | null | undefined, unit: string) =>
    v === null || v === undefined ? "-" : `${v.toLocaleString()} ${unit}`;

  const ROWS: [string, (r: Report) => string][] = [
    ["Cutoff", (r) => (r.measures.cutoffMedianHz ? formatHz(r.measures.cutoffMedianHz) : "-")],
    ["Bitrate", (r) => number(kbps(r), "kbps")],
    ["Verdict", (r) => VERDICTS[r.verdict]],
    ["Duration", (r) => formatDuration(r.durationSeconds)],
    ["Peak", (r) => (r.measures.peakDbfs == null ? "-" : `${r.measures.peakDbfs.toFixed(1)} dBFS`)],
    ["Sample rate", (r) => number(r.claimed.sampleRate, "Hz")]
  ];
</script>

<div class="overflow-x-auto rounded-card border bg-surface shadow-card">
  <table class="w-full text-sm tabular-nums">
    <caption class="sr-only">Figures of the files in slots A and B; differences are marked</caption>
    <thead>
      <tr class="border-b text-left text-xs text-fg-muted">
        <th scope="col" class="px-3 py-1.5 font-medium">Figure</th>
        <th scope="col" class="px-3 py-1.5 font-medium">A</th>
        <th scope="col" class="px-3 py-1.5 font-medium">B</th>
      </tr>
    </thead>
    <tbody>
      {#each ROWS as [label, value] (label)}
        {@const [first, second] = [value(a), value(b)]}
        <tr class="border-b last:border-b-0" data-differs={first !== second || undefined}>
          <th scope="row" class="px-3 py-1.5 text-left font-normal text-fg-muted">{label}</th>
          {#each [first, second] as cell, at (at)}
            <td class={["px-3 py-1.5", first !== second && "bg-accent-soft text-accent-soft-fg"]}
              >{cell}</td>
          {/each}
        </tr>
      {/each}
    </tbody>
  </table>
</div>
