/** How a type of notification is announced: the options' values, as the settings name them */
export const NOTIFY_MODES = [
  { id: "off", label: "Off" },
  { id: "sound", label: "Sound" },
  { id: "desktop", label: "Desktop" },
  { id: "both", label: "Both" }
] as const;

export type NotifyMode = (typeof NOTIFY_MODES)[number]["id"];

export const isNotifyMode = (value: unknown): value is NotifyMode =>
  NOTIFY_MODES.some((m) => m.id === value);

/** Whether `mode` plays the notification sound */
export const playsSound = (mode: NotifyMode) => mode === "sound" || mode === "both";

/** Whether `mode` shows a desktop notification, once the browser permits them */
export const showsDesktop = (mode: NotifyMode) => mode === "desktop" || mode === "both";
