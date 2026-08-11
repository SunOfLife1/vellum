{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = {
    self,
    nixpkgs,
  }: let
    pkgs = nixpkgs.legacyPackages.x86_64-linux;
    vellum = pkgs.rustPlatform.buildRustPackage {
      pname = "vellum";
      version = (fromTOML (builtins.readFile ./Cargo.toml)).package.version;
      src = self;
      cargoLock.lockFile = ./Cargo.lock;
      nativeBuildInputs = with pkgs; [pkg-config makeWrapper];
      buildInputs = with pkgs; [wayland wayland-protocols libxkbcommon libGL vulkan-loader];
      postInstall = ''
        wrapProgram $out/bin/vellum \
          --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (with pkgs; [libGL vulkan-loader])}
      '';
    };
  in {
    packages.x86_64-linux = {
      inherit vellum;
      default = vellum;
    };

    devShells.x86_64-linux.default = pkgs.mkShell {
      inputsFrom = [vellum];
      packages = with pkgs; [cargo rustc rustfmt clippy rust-analyzer];
    };

    homeModules.default = {
      config,
      lib,
      pkgs,
      ...
    }: let
      cfg = config.services.vellum;
    in {
      options.services.vellum = {
        enable = lib.mkEnableOption "Vellum screen annotation overlay";
        package = lib.mkOption {
          type = lib.types.package;
          default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
          description = "Vellum package to use.";
        };
        settings = lib.mkOption {
          type = lib.types.attrs;
          default = {};
          example = {
            default_tool = "pen";
            remember_last_tool = true;
            feedback_duration_ms = 500;
          };
          description = "Preferences written to vellum/config.toml.";
        };
      };

      config = lib.mkIf cfg.enable {
        home.packages = [cfg.package];
        xdg.configFile."vellum/config.toml" = lib.mkIf (cfg.settings != {}) {
          source = (pkgs.formats.toml {}).generate "vellum-config.toml" cfg.settings;
        };
        systemd.user.services.vellum = {
          Unit = {
            Description = "Vellum screen annotation overlay";
            After = ["graphical-session.target"];
            PartOf = ["graphical-session.target"];
          };
          Service = {
            ExecStart = "${cfg.package}/bin/vellum";
            Restart = "on-failure";
          };
          Install.WantedBy = ["graphical-session.target"];
        };
      };
    };
  };
}
