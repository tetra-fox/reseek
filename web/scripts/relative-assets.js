// Rewrites build/index.html to load its assets relative to the page, so one build works under
// any URL base the daemon is given.
//
// adapter-static writes the hash-router page as a fallback page, and @sveltejs/kit renders
// fallback pages with root-relative asset URLs (`/_app/...`) whatever `paths.relative` says (see
// `render_response` in kit's src/runtime/server/page/render.js, which only computes relative paths
// for pages that are not fallbacks). The page always lives at the base itself, so `./_app/` is
// right there.
// TODO: remove once kit writes relative asset paths for the hash router's page.
import { readFileSync, writeFileSync } from "node:fs";

const page = new URL("../build/index.html", import.meta.url);
const html = readFileSync(page, "utf8");
const relative = html.replaceAll('"/_app/', '"./_app/');
if (relative === html) {
  throw new Error(
    "build/index.html has no root-relative /_app/ paths; is this script still needed?"
  );
}
writeFileSync(page, relative);
