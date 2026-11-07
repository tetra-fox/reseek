import type { Crumb } from "#lib/nav.js";

/** The steps the screen on show adds under its own in the header's breadcrumb */
class Trail {
  page = $state.raw<Crumb[]>([]);
}

export const trail = new Trail();

/**
 * Puts the steps `make` gives under the screen's own in the breadcrumb, for as long as the
 * calling component is mounted, following whatever `make` reads. Call it while the component is
 * being set up
 */
export const useCrumbs = (make: () => Crumb[]) => {
  $effect(() => {
    const crumbs = make();
    trail.page = crumbs;
    // the steps go once the update that unmounted them is over, unless a component mounted in it
    // put its own in their place, so a hand-off never shows the breadcrumb without them between
    return () => {
      queueMicrotask(() => {
        if (trail.page === crumbs) trail.page = [];
      });
    };
  });
};
