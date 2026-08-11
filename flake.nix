{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = {
    self,
    nixpkgs,
  }: let
    pkgs = nixpkgs.legacyPackages.x86_64-linux;
    vellum = pkgs.callPackage ./package.nix {};
  in {
    packages.x86_64-linux = {
      inherit vellum;
      default = vellum;
    };

    devShells.x86_64-linux.default = pkgs.mkShell {
      inputsFrom = [vellum];
      packages = with pkgs; [cargo rustc rustfmt clippy];
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
