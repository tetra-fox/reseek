# `pnpm build` makes the asset paths relative, so one build works under any URL base
{
  lib,
  stdenvNoCC,
  nodejs_26,
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

  src = fs.toSource {
    root = ../web;
    fileset = fs.difference ../web (
      fs.unions [
        (fs.maybeMissing ../web/node_modules)
        (fs.maybeMissing ../web/build)
        (fs.maybeMissing ../web/.svelte-kit)
        (fs.maybeMissing ../web/test-results)
        (fs.maybeMissing ../web/playwright-report)
      ]
    );
  };

  pnpmDeps = fetchPnpmDeps {
    inherit (finalAttrs) pname version;
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
    hash = "sha256-6nk23+E18qkzZp6+BBLdQRmDDj3NzU7y1GSGyOiypbM=";
  };

  nativeBuildInputs = [
    nodejs_26
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
