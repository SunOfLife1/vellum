{self}: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.services.vellum;
  tomlFormat = pkgs.formats.toml {};
in {
  options.services.vellum = {
    enable = lib.mkEnableOption "Vellum screen annotation overlay";
    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      description = "Vellum package to use.";
    };
    settings = lib.mkOption {
      type = tomlFormat.type;
      default = {};
      example = {
        default_tool = "pen";
        remember_last_tool = true;
        default_fill_shapes = false;
        feedback_duration_ms = 500;
      };
      description = "Preferences written to vellum/config.toml.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [cfg.package];
    xdg.configFile."vellum/config.toml" = lib.mkIf (cfg.settings != {}) {
      source = tomlFormat.generate "vellum-config.toml" cfg.settings;
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
}
