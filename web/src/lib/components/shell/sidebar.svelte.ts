import { MediaQuery } from "svelte/reactivity";

import { settings } from "#lib/settings.svelte.js";

const narrow = new MediaQuery("max-width: 767px");

/** The navigation: a column on wide screens, expanded or icons only, and a sheet on phones */
class Sidebar {
  /** whether the phone sheet is open */
  sheetOpen = $state(false);

  get phone() {
    return narrow.current;
  }

  get collapsed() {
    return !this.phone && settings.sidebar === "collapsed";
  }

  toggle = () => {
    if (this.phone) this.sheetOpen = !this.sheetOpen;
    else settings.sidebar = settings.sidebar === "collapsed" ? "expanded" : "collapsed";
  };
}

export const sidebar = new Sidebar();
