type Settings = { sidebar: "expanded" | "collapsed" };

/** The one localStorage key the UI's own preferences live under */
const KEY = "reseek.ui.settings";

// per browser. Storage can be blocked (private windows, some policies), then the defaults are
// used and nothing is saved
const load = (): Settings => {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Settings>;
    return { sidebar: saved.sidebar === "collapsed" ? "collapsed" : "expanded" };
  } catch {
    return { sidebar: "expanded" };
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
