<script lang="ts">
  import type { Snippet } from "svelte";

  import { FLAGS, type Report, VERDICTS } from "#lib/analysis/analysis.js";
  import { formatDuration } from "#lib/format.js";

  // the analyser's own reading, under the spectrogram and folded away until asked for: its
  // verdict and summary, the cutoffs it found, every measure with what it tells, and what made
  // the reading less sure
  let {
    report,
    open = $bindable(false),
    children
  }: { report: Report; open?: boolean; children?: Snippet } = $props();

  const m = $derived(report.measures);
  const kHz = (hz: number | null | undefined) =>
    hz === null || hz === undefined ? null : `${(hz / 1000).toFixed(1)} kHz`;
  const db = (v: number | null | undefined) =>
    v === null || v === undefined ? null : `${v.toFixed(1)} dB`;
  const share = (v: number | null | undefined) =>
    v === null || v === undefined ? null : v.toFixed(2);
  /** each measure: its name, its value, and what it tells, left out when it has no value */
  const rows = $derived(
    (
      [
        ["Median cutoff", kHz(m.cutoffMedianHz), "Where the content usually stops."],
        [
          "Shelf sharpness",
          db(m.shelfSharpnessDbP95),
          "How steeply it stops: more than 30 dB is an encoder's low-pass, a few dB a natural roll-off."
        ],
        [
          "Gaps in the top band",
          share(m.sfb21Holes),
          "Empty bins between 16 and 20 kHz for each filled one. Reported only: it rises on tones too."
        ],
        [
          "Frame rhythm",
          share(m.frameArtefactScore),
          "How strongly the noise repeats at a lossy codec's frame length. Reported only."
        ],
        [
          "Energy above half the band",
          m.upsampleRatio === null || m.upsampleRatio === undefined
            ? null
            : m.upsampleRatio.toExponential(1),
          "For files above 48 kHz. Low for an upsample, and for much honest hi-res audio too."
        ],
        ["Bits in use", m.bitDepthEffective?.toString() ?? null, "How many bits the samples use."],
        ["Quietest level", db(m.noiseFloorDbfs), "The level of the quietest passages, in dBFS."],
        [
          "Channel correlation",
          share(m.stereoCorrelation),
          "1 means both channels are the same: a mono source."
        ],
        [
          "Side energy",
          db(m.sideEnergyDb),
          "The difference between the channels against their sum."
        ],
        [
          "Clipped samples",
          m.clippedSamplesPct === null || m.clippedSamplesPct === undefined
            ? null
            : `${m.clippedSamplesPct.toFixed(3)}%`,
          "Samples at full scale."
        ]
      ] as [string, string | null, string][]
    ).filter(([, value]) => value !== null)
  );
  const counted = $derived(m.cutoffHzHistogram.reduce((n, [, count]) => n + count, 0));
  const tallest = $derived(Math.max(1, ...m.cutoffHzHistogram.map(([, count]) => count)));
</script>

<details class="group rounded-control border" bind:open>
  <summary class="cursor-pointer px-3 py-2 text-sm font-medium">The analyser's reading</summary>
  <div class="flex flex-col gap-3 border-t px-3 py-3 text-sm">
    <p>
      <span class="font-medium">{VERDICTS[report.verdict]}</span>
      <span class="text-fg-muted">
        · {Math.round(report.confidence * 100)}% sure{report.encoderGuess
          ? ` · ${report.encoderGuess.profile}`
          : ""}</span>
    </p>
    <p>{report.summary}</p>
    {#if report.flags.length}
      <ul class="list-disc pl-5">
        {#each report.flags as flag (flag)}<li>{FLAGS[flag]}</li>{/each}
      </ul>
    {/if}
    {#if m.cutoffHzHistogram.length}
      <div class="flex flex-col gap-1">
        <h4 class="text-xs font-medium text-fg-muted uppercase">Where the content stops</h4>
        <ul class="flex flex-col gap-0.5" aria-label="Cutoffs by how many windows have them">
          {#each m.cutoffHzHistogram as [hz, count] (hz)}
            <li class="grid grid-cols-[5rem_1fr_3rem] items-center gap-2 text-xs">
              <span class="tabular-nums">{kHz(hz)}</span>
              <span class="rounded-sm h-2 bg-accent" style:width={`${(count / tallest) * 100}%`}
              ></span>
              <span class="text-right text-fg-muted tabular-nums"
                >{Math.round((count / counted) * 100)}%</span>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
    {@render children?.()}
    <div class="flex flex-col gap-1">
      <h4 class="text-xs font-medium text-fg-muted uppercase">Measures</h4>
      <dl class="flex flex-col gap-1.5">
        {#each rows as [name, value, meaning] (name)}
          <div>
            <dt class="flex justify-between gap-2">
              <span class="font-medium">{name}</span><span class="tabular-nums">{value}</span>
            </dt>
            <dd class="text-xs text-fg-muted">{meaning}</dd>
          </div>
        {/each}
      </dl>
    </div>
    <p class="text-xs text-fg-muted">
      {report.claimed.codec.toUpperCase()} · {report.claimed.sampleRate.toLocaleString()} Hz{report
        .claimed.bitDepth
        ? ` · ${report.claimed.bitDepth}-bit`
        : ""}{report.claimed.bitrateKbps ? ` · ${report.claimed.bitrateKbps} kbps` : ""} · {formatDuration(
        report.durationSeconds
      )}
    </p>
    {#if report.limitations.length}
      <div class="flex flex-col gap-1">
        <h4 class="text-xs font-medium text-fg-muted uppercase">What makes this less sure</h4>
        <ul class="list-disc pl-5 text-fg-muted">
          {#each report.limitations as limit (limit)}<li>{limit}</li>{/each}
        </ul>
      </div>
    {/if}
  </div>
</details>
