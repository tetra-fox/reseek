# reseek task runner. `just ci` is the gate for every push.

default:
    @just --list

# headless daemon with restart-on-change plus the vite ui with proxy
dev:
    @echo "dev: not yet (no daemon or web ui)"

# pnpm build, then cargo build --release (cargo never runs pnpm)
build: web-build
    cargo build --release

# only the ui into web/build
web-build:
    @echo "web-build: not yet (no web/)"

# cargo nextest, then vitest
test:
    cargo nextest run --workspace
    @echo "vitest: not yet (no web/)"

# fmt --check, clippy -D warnings, oxlint, oxfmt --check
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    @echo "oxlint, oxfmt: not yet (no web/)"

# regenerate api/openapi.json from the handlers
api-spec:
    @echo "api-spec: not yet (no api-types)"

# regenerate web/src/lib/api/schema.d.ts from api/openapi.json
api:
    @echo "api: not yet (no web/)"

# fail if the generated types are stale
api-check:
    @echo "api-check: not yet (no api/openapi.json)"

ci: lint test api-check

# the fuzz recipes need nightly cargo and cargo-fuzz: run them inside `nix develop .#fuzz`

# write the round-trip vectors into fuzz/corpus/<target> as seeds
fuzz-seed:
    cargo test -p soulseek-proto --test roundtrip -- --ignored --exact write_fuzz_corpus

# run every fuzz target for 30 seconds
fuzz-smoke: fuzz-seed
    for target in $(cargo fuzz list); do cargo fuzz run "$target" -- -max_total_time=30 || exit 1; done

# run one fuzz target, indefinitely unless SECONDS is given
fuzz TARGET SECONDS="": fuzz-seed
    cargo fuzz run {{ TARGET }} {{ if SECONDS == "" { "" } else { "-- -max_total_time=" + SECONDS } }}

# split HOURS evenly across every fuzz target
fuzz-all HOURS: fuzz-seed
    #!/usr/bin/env bash
    set -euo pipefail
    targets=$(cargo fuzz list)
    count=$(echo "$targets" | wc -l)
    per_target=$(( {{ HOURS }} * 3600 / count ))
    for target in $targets; do
      cargo fuzz run "$target" -- -max_total_time="$per_target"
    done
