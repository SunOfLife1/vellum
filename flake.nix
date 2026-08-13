{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = {
    self,
    nixpkgs,
  }: let
    systems = [ "aarch64-linux" "x86_64-linux" ];

    eachSystem = f: nixpkgs.lib.genAttrs systems
      (system: f system nixpkgs.legacyPackages.${system});
  in {
    packages = eachSystem (system: pkgs: {
      vellum = pkgs.callPackage ./nix/package.nix {};
      default = self.packages.${system}.vellum;
    });

    devShells = eachSystem (system: pkgs: {
      default = pkgs.mkShell {
        inputsFrom = [self.packages.${system}.vellum];
        packages = with pkgs; [
          cargo
          rustc
          rustfmt
          clippy
          rust-analyzer
        ];
        
        RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";

        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
          pkgs.libGL
          pkgs.vulkan-loader
          pkgs.wayland
          pkgs.libxkbcommon
        ];
      };
    });

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
