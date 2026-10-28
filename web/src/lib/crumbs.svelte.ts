import type { Crumb } from "./nav";

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
    return () => {
      if (trail.page === crumbs) trail.page = [];
    };
  });
};
