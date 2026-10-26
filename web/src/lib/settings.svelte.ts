/** How the UI keeps up with the daemon: the event stream, polling every so many ms, or not at all */
export type Refresh = "live" | number | "off";

export const REFRESH_OPTIONS: { value: Refresh; label: string }[] = [
  { value: "live", label: "Live" },
  { value: 5_000, label: "5s" },
  { value: 15_000, label: "15s" },
  { value: 60_000, label: "1m" },
  { value: "off", label: "Off" }
];

type Settings = { refresh: Refresh; sidebar: "expanded" | "collapsed" };

/** The one localStorage key the UI's own preferences live under */
const KEY = "reseek.ui.settings";
const defaults: Settings = { refresh: "live", sidebar: "expanded" };

// per browser. Storage can be blocked (private windows, some policies), then the defaults are
// used and nothing is saved
const load = (): Settings => {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Settings>;
    const refresh = REFRESH_OPTIONS.some((o) => o.value === saved.refresh)
      ? (saved.refresh as Refresh)
      : defaults.refresh;
    const sidebar = saved.sidebar === "collapsed" ? "collapsed" : defaults.sidebar;
    return { refresh, sidebar };
  } catch {
    return { ...defaults };
  }
};

export const settings = $state<Settings>(load());

$effect.root(() => {
  $effect(() => {
    const json = JSON.stringify(settings);
    try {
      localStorage.setItem(KEY, json);
    } catch {
      // not saved, the setting still holds until the page reloads
    }
  });
});
