{
  lib,
  rustPlatform,
  pkg-config,
  makeWrapper,
  wayland,
  wayland-protocols,
  libxkbcommon,
  libGL,
  vulkan-loader
}: rustPlatform.buildRustPackage {
  pname = "vellum";
  version = (fromTOML (builtins.readFile ./Cargo.toml)).package.version;
  src = "./";
  cargoLock.lockFile = ./Cargo.lock;
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
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath ([libGL vulkan-loader])}
  '';
}