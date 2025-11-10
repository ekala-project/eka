let
  lockstr = builtins.readFile ./atom.lock;
  lock = builtins.fromTOML lockstr;
  inherit (lock) locker;
  ekaSrc = builtins.head (builtins.filter (x: x.name == "eka") lock.deps);
  lockexpr =
    import
    <| builtins.fetchGit {
      inherit (locker) rev;
      name = locker.label;
      url = locker.mirror;
      ref = "refs/eka/atoms/${locker.label}/${locker.version}";
    };
in
lockexpr ./. lockstr {
  # FIXME: this is the only way to get the repository source into the atom for now
  # The long-term solution to this problem is specified in ../adrs/0010-electron-sources.md
  extraExtern.ext = rec {
    cargo-lock = import (src + "/build/Cargo.nix");
    src =
      if builtins.pathExists ../. then
        ../.
      else
        builtins.fetchGit {
          inherit (ekaSrc) name rev;
          url = ekaSrc.mirror;
        };
  };
  extraConfig = {
    platform = builtins.currentSystem or "x86_64-linux";
  };
}
