# reseek task runner

# the packages and features of every cargo build the gate makes: with one set, a build after
# `just test` finds everything compiled
tested := "--workspace --all-features"

# what the dev recipes run keeps its files here, gitignored: the daemon its config, data and logs,
# soulfind its database
devdir := justfile_directory() / ".dev"

default:
    @just --list

# in zellij_dev_layout.kdl, as a new tab when already in zellij; without zellij, both in this
# terminal
[doc("dev-daemon and dev-web side by side in zellij")]
dev: web-install
    #!/usr/bin/env bash
    set -euo pipefail
    layout="{{ justfile_directory() }}/zellij_dev_layout.kdl"
    if command -v zellij > /dev/null; then
      if [ -n "${ZELLIJ:-}" ]; then
        exec zellij action new-tab --layout "$layout" --cwd "{{ justfile_directory() }}" --name reseek
      fi
      exec zellij --layout "$layout"
    fi
    trap 'kill 0' EXIT
    {{ just_executable() }} --justfile {{ justfile() }} dev-daemon &
    {{ just_executable() }} --justfile {{ justfile() }} dev-web

# its flags are in crates/reseek-bin/dev-flags, which the cli tests start it with; its files are
# in .dev/daemon
[doc("the daemon without its built-in ui, restarted on change")]
dev-daemon:
    #!/usr/bin/env bash
    set -euo pipefail
    watchexec --restart --watch crates --exts rs,toml -- cargo run -p reseek-bin -- --app-dir "{{ devdir }}/daemon" $(cat crates/reseek-bin/dev-flags) &
    watch=$!
    # watchexec passes a hangup on to the daemon, which reloads its configuration on one, so a
    # closed terminal would leave both running; it stops watchexec here instead
    trap 'kill -TERM "$watch"' HUP
    wait "$watch" || wait "$watch"

# the vite ui, proxying the api to the daemon
dev-web: web-install
    pnpm --dir web dev

# pnpm build, then cargo build --release (cargo never runs pnpm)
build: web-build
    cargo build --release

# the daemon with the ui built in, and the relay agent
[doc("the static release binaries for linux x86_64 and aarch64 into dist/")]
release:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p dist
    for arch in amd64 arm64; do
      out="$(nix build --no-link --print-out-paths ".#release-$arch")"
      install -m755 "$out/bin/reseek" "dist/reseek-linux-$arch"
      install -m755 "$out/bin/reseek-agent" "dist/reseek-agent-linux-$arch"
    done

# the daemon with the ui built in, and the relay agent
[doc("the universal macos binaries into dist/, on a mac")]
release-macos: web-build
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p dist
    for target in x86_64-apple-darwin aarch64-apple-darwin; do
      rustup target add "$target"
      cargo build --release -p reseek-bin --target "$target"
    done
    for program in reseek reseek-agent; do
      lipo -create -output "dist/$program-macos-universal" \
        "target/x86_64-apple-darwin/release/$program" "target/aarch64-apple-darwin/release/$program"
    done

# the container image `reseek` from the release binaries, with podman or docker
docker: release
    #!/usr/bin/env bash
    set -euo pipefail
    runtime="$(command -v podman || command -v docker)"
    context="$(mktemp -d)"
    trap 'rm -rf "$context"' EXIT
    for arch in amd64 arm64; do cp "dist/reseek-linux-$arch" "$context/reseek-$arch"; done
    cp docker/entrypoint.sh "$context/"
    # podman writes OCI images by default, which have no HEALTHCHECK
    format=()
    if [ "$(basename "$runtime")" = podman ]; then format=(--format docker); fi
    "$runtime" build "${format[@]}" -f docker/Dockerfile -t reseek "$context"

# only the ui into web/build
web-build: web-install
    pnpm --dir web build

# cargo nextest, the doc examples nextest does not run, then vitest
test: web-install
    cargo nextest run {{ tested }}
    cargo test {{ tested }} --doc
    @[ -n "$({{ just_executable() }} --justfile {{ justfile() }} soulfind-bin)" ] || echo "soulfind not found at SOULFIND_BIN or on PATH: the soulfind suites were skipped"
    pnpm --dir web test

# fmt --check, clippy -D warnings, oxlint, oxfmt --check, svelte-check, and the UI's API types
# and settings fixture still what the daemon generates
[doc("formatting, clippy, the UI's linters and types, and the generated files up to date")]
lint: web-install
    cargo fmt --all --check
    cargo fmt --manifest-path fuzz/Cargo.toml --all --check
    cargo clippy {{ tested }} --all-targets -- -D warnings
    pnpm --dir web lint
    pnpm --dir web check
    pnpm --dir web api-check
    @view="$({{ just_executable() }} --justfile {{ justfile() }} config-view)" \
      || { echo "the settings fixture could not be generated" >&2; exit 1; }; \
      printf '%s\n' "$view" | diff -q web/src/lib/config/fixtures/view.json - > /dev/null \
      || { echo "web/src/lib/config/fixtures/view.json is stale, run \`just config-fixture\`" >&2; exit 1; }

# regenerate api/openapi.json from the handlers, then web/src/lib/api/schema.d.ts from it
api: web-install
    cargo run -q -p reseek --example openapi > api/openapi.json
    pnpm --dir web api

# regenerate web/src/lib/config/fixtures/view.json, the settings view of the default configuration
config-fixture:
    {{ just_executable() }} --justfile {{ justfile() }} config-view > web/src/lib/config/fixtures/view.json

# prints the settings view of the default configuration, from the example built in the test
# profile, where `just test` has compiled everything it needs
[private]
config-view:
    @cargo build -q {{ tested }} --profile test --example config_view
    @"${CARGO_TARGET_DIR:-target}/debug/examples/config_view"

# the built UI served by the daemon, driven in a browser
web-e2e: (web-playwright "e2e")

# the Analysis page's frame-rate benchmarks in a browser, run by hand rather than in ci
web-perf: (web-playwright "perf")

# builds the UI and the daemon and runs the web package's playwright script SCRIPT, skipped
# without chromium. The daemon is the one the test build writes for the integration tests: a
# plain cargo build leaves out the dev-dependencies, whose features change what it compiles
[private]
web-playwright SCRIPT:
    #!/usr/bin/env bash
    set -euo pipefail
    chromium="${CHROMIUM_BIN:-$(command -v chromium || true)}"
    if [ -z "$chromium" ]; then
      echo "web-{{ SCRIPT }} skipped: no chromium at CHROMIUM_BIN or on PATH"
      exit 0
    fi
    {{ just_executable() }} --justfile {{ justfile() }} web-build
    cargo nextest run {{ tested }} --no-run
    CHROMIUM_BIN="$chromium" pnpm --dir web {{ SCRIPT }}

# the web UI's packages as the lockfile pins them
[private]
web-install:
    pnpm --dir web install --frozen-lockfile --prefer-offline --silent

# prints the soulfind to test against: SOULFIND_BIN, else soulfind on PATH, else nothing, which
# the callers take as a skip; with SOULFIND_REQUIRED=1 a missing soulfind fails instead
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
ci: lint test web-e2e

# soulfind on its default port 2242 with its data under .dev/soulfind, for trying things by hand
soulfind:
    mkdir -p "{{ devdir }}/soulfind"
    "${SOULFIND_BIN:-soulfind}" -p 2242 -d "{{ devdir }}/soulfind/soulfind.db"

# the fuzz recipes need nightly cargo and cargo-fuzz: run them inside `nix develop .#fuzz`

# and an ALAC file for the WAV target
[doc("write the round-trip vectors into fuzz/corpus/<target> as seeds")]
fuzz-seed:
    cargo test -p soulseek-proto --lib -- --ignored --exact testing::write_fuzz_corpus
    mkdir -p fuzz/corpus/wav_stream
    cp crates/reseek/src/files/fixtures/tone.m4a fuzz/corpus/wav_stream/

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
