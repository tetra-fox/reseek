<script lang="ts">
  import type { Report } from "#lib/analysis/analysis.js";
  import { VERDICTS } from "#lib/analysis/analysis.js";
  import { formatHz } from "#lib/analysis/spectrogram/frequency.js";
  import { formatCount, formatDuration } from "#lib/format.js";

  // two reports' main figures side by side, a row whose values differ marked. It states numbers
  // and picks neither file
  type Props = { a: Report; b: Report };

  let { a, b }: Props = $props();

  /** the stream's bitrate for a lossy file, for a lossless one the file's size over its length */
  const kbps = (r: Report) =>
    r.claimed.bitrateKbps ??
    (r.durationSeconds > 0 ? Math.round((r.size * 8) / r.durationSeconds / 1000) : null);
  /** a figure with its unit; a file that could not be read claims 0 for each, which is none */
  const number = (v: number | null | undefined, unit: string) =>
    v ? `${formatCount(v)} ${unit}` : "-";

  const ROWS: [string, (r: Report) => string][] = [
    ["Cutoff", (r) => (r.measures.cutoffMedianHz ? formatHz(r.measures.cutoffMedianHz) : "-")],
    ["Bitrate", (r) => number(kbps(r), "kbps")],
    ["Verdict", (r) => VERDICTS[r.verdict]],
    ["Duration", (r) => (r.durationSeconds > 0 ? formatDuration(r.durationSeconds) : "-")],
    ["Peak", (r) => (r.measures.peakDbfs == null ? "-" : `${r.measures.peakDbfs.toFixed(1)} dBFS`)],
    ["Sample rate", (r) => number(r.claimed.sampleRate, "Hz")]
  ];
</script>

<div class="overflow-x-auto">
  <table class="data-table dense w-full text-sm tabular-nums">
    <caption class="sr-only">Figures of the files in slots A and B; differences are marked</caption>
    <thead>
      <tr>
        <th scope="col">Figure</th>
        <th scope="col">A</th>
        <th scope="col">B</th>
      </tr>
    </thead>
    <tbody>
      {#each ROWS as [label, value] (label)}
        {@const [first, second] = [value(a), value(b)]}
        <tr data-differs={first !== second || undefined}>
          <!-- labels and values wrap, so both columns fit a phone's popover -->
          <th scope="row" class="text-sm font-normal whitespace-normal">{label}</th>
          {#each [first, second] as cell, at (at)}
            <td
              class={[
                "whitespace-normal",
                first !== second && "bg-accent-soft text-accent-soft-fg"
              ]}>{cell}</td>
          {/each}
        </tr>
      {/each}
    </tbody>
  </table>
</div>
