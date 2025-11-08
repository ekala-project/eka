let
  inherit (deps) pins;
in
{
  Shell = mod.shell;
  Pkgs = mod.pkgs;
  pkgs = pins.nixpkgs.import "" { system = cfg.platform; };
  fenix = pins.fenix.import "" {
    system = cfg.platform;
    inherit (mod) pkgs;
  };
}
