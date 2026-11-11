<script lang="ts">
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { StreamLanguage } from "@codemirror/language";
  import { toml } from "@codemirror/legacy-modes/mode/toml";
  import { EditorState } from "@codemirror/state";
  import { EditorView, keymap, lineNumbers } from "@codemirror/view";
  import { untrack } from "svelte";

  import { cn } from "#lib/ui/cn.js";

  type Props = {
    "value": string;
    "readonly"?: boolean;
    /** called with the whole text after each edit */
    "onchange"?: (text: string) => void;
    "aria-label": string;
    "class"?: string;
  };

  let {
    value,
    readonly = false,
    onchange,
    "aria-label": label,
    "class": className
  }: Props = $props();

  let view: EditorView | null = null;

  // colors from the page's tokens, so the editor follows the light and dark themes
  const theme = EditorView.theme({
    "&": { backgroundColor: "var(--color-surface)", color: "var(--color-fg)", height: "100%" },
    ".cm-content": { fontFamily: "var(--font-mono)", caretColor: "var(--color-fg)" },
    ".cm-gutters": {
      backgroundColor: "var(--color-surface-muted)",
      color: "var(--color-fg-muted)",
      border: "none"
    },
    "&.cm-focused": { outline: "none" },
    ".cm-activeLine": { backgroundColor: "transparent" },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
      backgroundColor: "var(--color-accent-soft)"
    }
  });

  // made once: the attachment would make it again whenever a value it reads changed
  const mount = (node: HTMLElement) => {
    const editor = untrack(() => create(node));
    view = editor;
    return () => {
      editor.destroy();
      view = null;
    };
  };

  const create = (node: HTMLElement) =>
    new EditorView({
      parent: node,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          history(),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          StreamLanguage.define(toml),
          theme,
          EditorState.readOnly.of(readonly),
          EditorView.editable.of(!readonly),
          EditorView.contentAttributes.of({ "aria-label": label }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) onchange?.(update.state.doc.toString());
          })
        ]
      })
    });

  // a new value from outside replaces the text, unless it is the text already shown
  $effect(() => {
    const next = value;
    if (view && view.state.doc.toString() !== next) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: next } });
    }
  });
</script>

<div class={cn("overflow-auto rounded-control border text-sm", className)} {@attach mount}></div>
