let
  lockstr = builtins.readFile ./atom.lock;
  lock = builtins.fromTOML lockstr;
  inherit (lock) locker;
  lockexpr =
    import
    <| (builtins.fetchGit {
      inherit (locker) rev;
      name = locker.label;
      url = locker.mirror;
      ref = "refs/eka/atoms/${locker.label}/${locker.version}";
    }) + "/atom.nix";
in
lockexpr {
  root = ./.;
  config = {
    platforms.build = builtins.currentSystem or "x86_64-linux";
    platforms.target = builtins.currentSystem or "x86_64-linux";
  };
}
