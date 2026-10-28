# reseek task runner. `just ci` is the gate for every push.

default:
    @just --list

# the daemon without its built-in ui, restarted on change, plus the vite ui with proxy
dev: web-install
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'kill 0' EXIT
    watchexec --restart --watch crates --exts rs,toml -- cargo run -p reseek -- --web-ui=false &
    pnpm --dir web dev

# pnpm build, then cargo build --release (cargo never runs pnpm)
build: web-build
    cargo build --release

# only the ui into web/build
web-build: web-install
    pnpm --dir web build

# cargo nextest, the doc examples nextest does not run, then vitest
test: web-install
    cargo nextest run --workspace --all-features
    cargo test --workspace --all-features --doc
    @[ -n "$({{ just_executable() }} --justfile {{ justfile() }} soulfind-bin)" ] || echo "soulfind not found at SOULFIND_BIN or on PATH: the soulfind suites were skipped"
    pnpm --dir web test

# fmt --check, clippy -D warnings, oxlint, oxfmt --check, svelte-check
lint: web-install
    cargo fmt --all --check
    cargo fmt --manifest-path fuzz/Cargo.toml --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    pnpm --dir web lint
    pnpm --dir web check

# regenerate api/openapi.json from the handlers
api-spec:
    cargo run -q -p reseek --example openapi > api/openapi.json

# regenerate web/src/lib/api/schema.d.ts from api/openapi.json
api: web-install
    pnpm --dir web api

# regenerate web/src/lib/config/fixtures/view.json, the settings view of the default configuration
config-fixture:
    {{ just_executable() }} --justfile {{ justfile() }} config-view > web/src/lib/config/fixtures/view.json

# prints the settings view of the default configuration
[private]
config-view:
    @cargo run -q -p reseek --example config_view

# fail if api/openapi.json no longer matches the handlers, the UI's types no longer match it, or
# the settings fixture no longer matches the daemon's options
api-check: web-install
    cargo nextest run -p reseek -E "test(the_committed_openapi_document_is_current)"
    pnpm --dir web api-check
    @view="$({{ just_executable() }} --justfile {{ justfile() }} config-view)" \
      || { echo "the settings fixture could not be generated" >&2; exit 1; }; \
      printf '%s\n' "$view" | diff -q web/src/lib/config/fixtures/view.json - > /dev/null \
      || { echo "web/src/lib/config/fixtures/view.json is stale, run \`just config-fixture\`" >&2; exit 1; }

# the built UI served by the daemon in a browser: it loads, logs in and moves between screens
web-smoke:
    #!/usr/bin/env bash
    set -euo pipefail
    chromium="${CHROMIUM_BIN:-$(command -v chromium || true)}"
    if [ -z "$chromium" ]; then
      echo "web-smoke skipped: no chromium at CHROMIUM_BIN or on PATH"
      exit 0
    fi
    {{ just_executable() }} --justfile {{ justfile() }} web-build
    cargo build -p reseek
    CHROMIUM_BIN="$chromium" pnpm --dir web smoke

# the web UI's packages as the lockfile pins them
[private]
web-install:
    pnpm --dir web install --frozen-lockfile --prefer-offline --silent

# soulfind and the soulseek-rs cli as the other user: search, browse and transfers both ways, by
# the client and by the daemon through its api
e2e:
    #!/usr/bin/env bash
    set -euo pipefail
    soulfind="$({{ just_executable() }} --justfile {{ justfile() }} soulfind-bin)"
    soulseek_rs="${SOULSEEK_RS_BIN:-$(command -v soulseek-rs || true)}"
    if [ -z "$soulfind" ] || [ -z "$soulseek_rs" ]; then
      echo "e2e skipped: soulfind (SOULFIND_BIN or PATH) or soulseek-rs (SOULSEEK_RS_BIN or PATH) not found"
      exit 0
    fi
    export SOULFIND_BIN="$soulfind" SOULSEEK_RS_BIN="$soulseek_rs"
    cargo nextest run -p soulseek-client --test interop --run-ignored only
    cargo nextest run -p reseek --test smoke --run-ignored only -E "test(soulseek_rs)"

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
ci: lint test api-check web-smoke e2e live-smoke-local

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
    #!/usr/bin/env bash
    set -euo pipefail
    # without cargo-fuzz the list fails, so the smoke fails rather than running nothing
    targets=$(cargo fuzz list)
    for target in $targets; do cargo fuzz run "$target" -- -max_total_time=30; done

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
