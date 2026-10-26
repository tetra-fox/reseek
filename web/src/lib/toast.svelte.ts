export type ToastAction = { label: string; run: () => void };
export type Toast = { id: number; text: string; kind: "info" | "error"; action?: ToastAction };

export const toasts = $state<Toast[]>([]);

let next = 0;

export const dismiss = (id: number) => {
  const i = toasts.findIndex((t) => t.id === id);
  if (i !== -1) toasts.splice(i, 1);
};

/** Shows `text` for a while, errors for longer. An action's button dismisses it when pressed */
export const toast = (
  text: string,
  kind: Toast["kind"] = "info",
  { action, ms = kind === "error" ? 12_000 : 6_000 }: { action?: ToastAction; ms?: number } = {}
) => {
  const id = next++;
  toasts.unshift({ id, text, kind, action });
  setTimeout(() => dismiss(id), ms);
  return id;
};
