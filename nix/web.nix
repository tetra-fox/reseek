# The web UI as the daemon serves it: `pnpm build` in web/, which also makes the page's asset
# paths relative so one build works under any URL base. The Rust package embeds this output.
{
  lib,
  stdenvNoCC,
  nodejs_24,
  pnpm,
  fetchPnpmDeps,
  pnpmConfigHook,
  version,
}:
let
  fs = lib.fileset;
in
stdenvNoCC.mkDerivation (finalAttrs: {
  pname = "reseek-web";
  inherit version;

  # api/openapi.json beside web/, for the check that the UI's types match it
  src = fs.toSource {
    root = ../.;
    fileset = fs.unions [
      ../api/openapi.json
      (fs.difference ../web (
        fs.unions [
          (fs.maybeMissing ../web/node_modules)
          (fs.maybeMissing ../web/build)
          (fs.maybeMissing ../web/.svelte-kit)
          (fs.maybeMissing ../web/test-results)
          (fs.maybeMissing ../web/playwright-report)
        ]
      ))
    ];
  };
  sourceRoot = "${finalAttrs.src.name}/web";

  pnpmDeps = fetchPnpmDeps {
    # named apart from `pname`, so the checks built from this derivation share the packages
    pname = "reseek-web";
    inherit (finalAttrs) version;
    inherit pnpm;
    # the packages depend on the manifest and the lockfile alone
    src = fs.toSource {
      root = ../web;
      fileset = fs.unions [
        ../web/package.json
        ../web/pnpm-lock.yaml
      ];
    };
    fetcherVersion = 4;
    hash = "sha256-Qwrr1ORlNXlwnlN3OlEjSccMeegfg1XprgoIjYHFChA=";
  };

  nativeBuildInputs = [
    nodejs_24
    pnpm
    pnpmConfigHook
  ];

  buildPhase = ''
    runHook preBuild
    pnpm build
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    cp -r build $out
    runHook postInstall
  '';

  meta = {
    description = "The reseek web UI, built";
    license = lib.licenses.agpl3Only;
  };
})
