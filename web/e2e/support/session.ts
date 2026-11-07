import { fileURLToPath } from "node:url";

/**
 * Where the login project keeps the session cookie the tests start with. Under test-results,
 * which playwright empties at the start of a run, before the login runs
 */
export const SESSION = fileURLToPath(new URL("../../test-results/session.json", import.meta.url));
