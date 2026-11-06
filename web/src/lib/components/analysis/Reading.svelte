<script lang="ts">
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";

  import { FLAGS, type Report, VERDICTS } from "#lib/analysis/analysis.js";
  import { formatDuration } from "#lib/format.js";
  import { disclose } from "#lib/ui/motion.js";

  // the analyzer's own reading, under the spectrogram and folded away until asked for: its
  // verdict and summary, the cutoffs it found, every measure with what it tells, and what made
  // the reading less sure
  let { report, open = $bindable(false) }: { report: Report; open?: boolean } = $props();

  const m = $derived(report.measures);
  const kHz = (hz: number | null | undefined) =>
    hz === null || hz === undefined ? null : `${(hz / 1000).toFixed(1)} kHz`;
  const db = (v: number | null | undefined) =>
    v === null || v === undefined ? null : `${v.toFixed(1)} dB`;
  const share = (v: number | null | undefined) =>
    v === null || v === undefined ? null : v.toFixed(2);
  /** each measure: its name, its value and what its name doesn't tell; left out without a value */
  const rows = $derived(
    (
      [
        ["Median cutoff", kHz(m.cutoffMedianHz), "Where the content usually ends"],
        [
          "Shelf sharpness",
          db(m.shelfSharpnessDbP95),
          "Above 30 dB points to an encoder low-pass, a few dB to a natural roll-off"
        ],
        [
          "Gaps in the top band",
          share(m.sfb21Holes),
          "Empty bins per filled bin from 16 to 20 kHz. Tonal content raises it too, so it is not used in the verdict."
        ],
        [
          "Frame rhythm",
          share(m.frameArtifactScore),
          "Noise repeating at a lossy codec's frame length. Not used in the verdict."
        ],
        [
          "Energy above half the band",
          m.upsampleRatio === null || m.upsampleRatio === undefined
            ? null
            : m.upsampleRatio.toExponential(1),
          "Measured for sample rates above 48 kHz. Low for upsampled audio, but also for much genuine high-resolution audio."
        ],
        ["Bits in use", m.bitDepthEffective?.toString() ?? null, ""],
        ["Quietest level", db(m.noiseFloorDbfs), "The noise floor, in dBFS"],
        [
          "Channel correlation",
          share(m.stereoCorrelation),
          "At 1 the channels are identical, so the source is mono"
        ],
        [
          "Side energy",
          db(m.sideEnergyDb),
          "Difference between the channels relative to their sum"
        ],
        [
          "Clipped samples",
          m.clippedSamplesPct === null || m.clippedSamplesPct === undefined
            ? null
            : `${m.clippedSamplesPct.toFixed(3)}%`,
          ""
        ]
      ] as [string, string | null, string][]
    ).filter(([, value]) => value !== null)
  );
  const counted = $derived(m.cutoffHzHistogram.reduce((n, [, count]) => n + count, 0));
  const tallest = $derived(Math.max(1, ...m.cutoffHzHistogram.map(([, count]) => count)));
</script>

<section class="rounded-control border">
  <button
    type="button"
    class="flex w-full items-center gap-1.5 px-3 py-2 text-left text-sm font-medium"
    aria-expanded={open}
    onclick={() => (open = !open)}>
    <ChevronRightIcon
      class={[
        "size-4 text-fg-muted transition-transform duration-(--motion-fast)",
        open && "rotate-90"
      ]} />
    Analyzer reading
  </button>
  {#if open}
    <div class="flex flex-col gap-3 border-t px-3 py-3 text-sm" transition:disclose>
      <p>
        <span class="font-medium">{VERDICTS[report.verdict]}</span>
        <span class="text-fg-muted">
          · {Math.round(report.confidence * 100)}% confidence{report.encoderGuess
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
          <h4 class="text-xs font-medium text-fg-muted">Cutoff distribution</h4>
          <ul class="flex flex-col gap-0.5" aria-label="Cutoffs by share of windows">
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
      <div class="flex flex-col gap-1">
        <h4 class="text-xs font-medium text-fg-muted">Measures</h4>
        <dl class="flex flex-col gap-1.5">
          {#each rows as [name, value, meaning] (name)}
            <div>
              <dt class="flex justify-between gap-2">
                <span class="font-medium">{name}</span><span class="tabular-nums">{value}</span>
              </dt>
              {#if meaning}<dd class="text-xs text-fg-muted">{meaning}</dd>{/if}
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
          <h4 class="text-xs font-medium text-fg-muted">Limitations</h4>

          <ul class="list-disc pl-5 text-fg-muted">
            {#each report.limitations as limit (limit)}<li>{limit}</li>{/each}
          </ul>
        </div>
      {/if}
    </div>
  {/if}
</section>
