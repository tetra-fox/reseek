<script lang="ts">
  import { Tooltip } from "bits-ui";
  import { type Snippet, tick } from "svelte";
  import { createAttachmentKey } from "svelte/attachments";
  import { on } from "svelte/events";

  import { type Side, tether, words } from "#lib/components/ui/TooltipHost.svelte";
  import type { Binding } from "#lib/ui/keys.js";

  type Props = {
    /**
     * no tooltip when it's missing. A snippet draws words that hold what plain text can't, such
     * as a username in its own look
     */
    text: string | Snippet | undefined;
    /** which side of the element it opens on */
    side?: Side;
    /** the keys that do what the element does, named after the text */
    keys?: Binding;
    /**
     * for a static element in a dense list, such as a timestamp on every line: it stays out of the
     * tab order. Any other static element with a tooltip is a tab stop, so the keys can show it
     */
    dense?: boolean;
    /** repeats the text for screen readers, for an element that says nothing of it otherwise */
    spoken?: boolean;
    /**
     * for text that is its own full text: the tooltip shows only while the element cuts it off,
     * and the element stays out of the tab order unless it is focusable itself
     */
    clipped?: boolean;
    /**
     * the element renders without a trigger while true: a row of a long list sets it until a
     * mouse or a pen first comes to the row (`wakeOnPointer`), since each trigger joins bits-ui's
     * registry. Keyboard focus wakes the one element, which is the tab stop its trigger would
     * make it
     */
    dormant?: boolean;
    /** the element the tooltip belongs to, which spreads `props` */
    child: Snippet<[{ props: Record<string, unknown> }]>;
  };

  let {
    text,
    side = "top",
    keys,
    dense = false,
    spoken = false,
    clipped = false,
    dormant = false,
    child: render
  }: Props = $props();

  let cut = $state(false);

  const cuts = (e: Element) => e.scrollWidth > e.clientWidth || e.scrollHeight > e.clientHeight;

  // measured as the pointer or focus arrives, in the capture phase so it is known before the
  // trigger's own handlers decide whether to open. The text may be cut by a part inside the
  // element rather than the element itself, as a text cut in the middle is
  const measure = (node: HTMLElement) => {
    const check = () => {
      cut = cuts(node) || [...node.querySelectorAll("*")].some(cuts);
    };
    node.addEventListener("pointerenter", check, { capture: true });
    node.addEventListener("focus", check, { capture: true });
    return () => {
      node.removeEventListener("pointerenter", check, { capture: true });
      node.removeEventListener("focus", check, { capture: true });
    };
  };
  const measuring = { [createAttachmentKey()]: measure };

  // keyboard focus on a dormant element mounts its trigger, which renders the element anew, so
  // the new one takes the focus back. A press's focus wakes nothing: the swap would lose its click
  let woken = $state(false);
  let refocus = false;
  const waking = {
    [createAttachmentKey()]: (node: HTMLElement) =>
      on(node, "focus", () => {
        if (node.matches(":focus-visible")) woken = refocus = true;
      })
  };
  const taking = {
    [createAttachmentKey()]: (node: HTMLElement) => {
      if (!refocus) return;
      refocus = false;
      // once the trigger has joined the tether, which its focus opens the tooltip on: before
      // that the tether falls back to its first trigger and shows that one's text
      void tick().then(() => node.focus());
    }
  };
  // the tab stop the trigger gives a static element, which bits-ui leaves off a disabled one
  const stop = $derived(dense || clipped || text === undefined ? {} : { tabindex: 0 });
</script>

<!-- a trigger once awake, disabled without text, so the element isn't remounted when a tooltip
     comes and goes. The spoken text follows it without a line break, which would render as a space -->
{#if dormant && !woken}{@render render({ props: { ...stop, ...waking } })}{:else}<Tooltip.Trigger
    {tether}
    payload={{ text: text ?? "", side, keys }}
    disabled={text === undefined || (clipped && !cut)}>
    {#snippet child({ props })}
      <!-- type is for bits-ui's own button, and disabled only means there's nothing to show -->
      {@const {
        "type": _type,
        "disabled": _disabled,
        "data-disabled": _dataDisabled,
        "tabindex": tabindex,
        ...own
      } = props}
      <!-- the trigger makes its element a tab stop, which dense and clipped leave to the element -->
      {@render render({
        props: {
          ...own,
          ...(dense || clipped ? {} : { tabindex }),
          ...(clipped ? measuring : {}),
          ...(woken ? taking : {})
        }
      })}
    {/snippet}
  </Tooltip.Trigger>{/if}{#if spoken && text !== undefined}<span class="sr-only"
    >{@render words(text)}</span
  >{/if}
