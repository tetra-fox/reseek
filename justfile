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

# cargo nextest, the doc examples nextest does not run, then vitest
test:
    cargo nextest run --workspace --all-features
    cargo test --workspace --all-features --doc
    @[ -n "$({{ just_executable() }} --justfile {{ justfile() }} soulfind-bin)" ] || echo "soulfind not found at SOULFIND_BIN or on PATH: the soulfind suites were skipped"
    @echo "vitest: not yet (no web/)"

# fmt --check, clippy -D warnings, oxlint, oxfmt --check
lint:
    cargo fmt --all --check
    cargo fmt --manifest-path fuzz/Cargo.toml --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
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

# soulfind and the soulseek-rs cli as the other user: search, browse and transfers both ways
e2e:
    #!/usr/bin/env bash
    set -euo pipefail
    soulfind="$({{ just_executable() }} --justfile {{ justfile() }} soulfind-bin)"
    soulseek_rs="${SOULSEEK_RS_BIN:-$(command -v soulseek-rs || true)}"
    if [ -z "$soulfind" ] || [ -z "$soulseek_rs" ]; then
      echo "e2e skipped: soulfind (SOULFIND_BIN or PATH) or soulseek-rs (SOULSEEK_RS_BIN or PATH) not found"
      exit 0
    fi
    SOULFIND_BIN="$soulfind" SOULSEEK_RS_BIN="$soulseek_rs" \
      cargo nextest run -p soulseek-client --test interop --run-ignored only

# the live smoke example against a local soulfind, with other users sharing the file it fetches
live-smoke-local:
    #!/usr/bin/env bash
    set -euo pipefail
    soulfind="$({{ just_executable() }} --justfile {{ justfile() }} soulfind-bin)"
    if [ -z "$soulfind" ]; then
      echo "live-smoke-local skipped: soulfind (SOULFIND_BIN or PATH) not found"
      exit 0
    fi
    cargo build -p soulseek-client --example live_smoke
    SOULFIND_BIN="$soulfind" \
      cargo nextest run -p soulseek-client --test live_smoke --run-ignored only

# Run by hand only, never from ci or by an agent: it logs in to the real Soulseek server with
# the throwaway account in RESEEK_TEST_USERNAME and RESEEK_TEST_PASSWORD. --help lists the rest
[doc("the live smoke example against the real Soulseek server, run by hand only")]
[positional-arguments]
live-smoke *ARGS:
    cargo run -p soulseek-client --example live_smoke -- "$@"

# Prints the soulfind to test against: SOULFIND_BIN, else soulfind on PATH, else nothing, which
# the callers take as a skip. With SOULFIND_REQUIRED=1 a missing soulfind fails instead
[private]
soulfind-bin:
    #!/usr/bin/env bash
    set -euo pipefail
    soulfind="${SOULFIND_BIN:-$(command -v soulfind || true)}"
    if [ -z "$soulfind" ] && [ "${SOULFIND_REQUIRED:-}" = 1 ]; then
      echo "SOULFIND_REQUIRED=1 but soulfind is neither at SOULFIND_BIN nor on PATH" >&2
      exit 1
    fi
    echo "$soulfind"

# the gate for every push
ci: lint test api-check e2e live-smoke-local

# soulfind on its default port 2242 with its data under target/soulfind, for trying things by hand
soulfind:
    mkdir -p target/soulfind
    "${SOULFIND_BIN:-soulfind}" -p 2242 -d target/soulfind/soulfind.db

# the fuzz recipes need nightly cargo and cargo-fuzz: run them inside `nix develop .#fuzz`

# write the round-trip vectors into fuzz/corpus/<target> as seeds
fuzz-seed:
    cargo test -p soulseek-proto --lib -- --ignored --exact testing::write_fuzz_corpus

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
