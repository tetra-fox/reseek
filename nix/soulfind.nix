# the Soulseek server the client and daemon test suites run against; nothing in reseek links to it
{
  lib,
  buildDubPackage,
  fetchFromGitHub,
  pkg-config,
  sqlite,
}:
buildDubPackage {
  pname = "soulfind";
  version = "0-unstable-2026-10-15";

  src = fetchFromGitHub {
    owner = "soulfind-dev";
    repo = "soulfind";
    rev = "5a256f5c9845d362c2bfbdcb26a8355ad1f395fc";
    hash = "sha256-3L4a3o0XmYtfXEmOl33Z1Dd6ucPFxF2FHjwdUur280k=";
  };

  dubLock.dependencies = { };

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ sqlite ];

  installPhase = ''
    runHook preInstall
    install -Dm755 bin/soulfind bin/soulsetup -t $out/bin
    runHook postInstall
  '';

  meta = {
    description = "Soulseek server for local testing";
    homepage = "https://github.com/soulfind-dev/soulfind";
    license = lib.licenses.gpl3Plus;
    mainProgram = "soulfind";
  };
}
