{
  lib,
  rustPlatform,
  pkg-config,
  makeWrapper,
  wayland,
  wayland-protocols,
  libxkbcommon,
  libGL,
  vulkan-loader,
}:
let
  pname = "vellum";
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
    ];
  };

  cargoToml = (builtins.fromTOML (builtins.readFile "${src}/Cargo.toml"));
in rustPlatform.buildRustPackage {
  inherit pname src;
  version = cargoToml.package.version;

  cargoLock.lockFile = "${src}/Cargo.lock";

  nativeBuildInputs = [
    pkg-config
    makeWrapper
  ];

  buildInputs = [
    wayland
    wayland-protocols
    libxkbcommon
    libGL
    vulkan-loader
  ];

  postInstall = ''
    wrapProgram $out/bin/vellum \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath [libGL vulkan-loader]}
  '';
}