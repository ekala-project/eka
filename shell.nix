let
  dev = import ./dev {
    extraConfig = {
      platform = builtins.currentSystem or "x86_64-linux";
    };
  };
in
dev.shell
