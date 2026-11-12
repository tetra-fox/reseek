import type fixture from "./fixtures/view.json";

/**
 * A key of the daemon's options, as `just config-fixture` last wrote them, so a key the daemon
 * renamed or dropped fails the type check
 */
export type OptionKey = keyof (typeof fixture)["fields"];
