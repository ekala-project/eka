let
  lockstr = builtins.readFile ./atom.lock;
  lock = builtins.fromTOML lockstr;
  inherit (lock) locker;
  lockexpr =
    import
    <| builtins.fetchGit {
      inherit (locker) rev;
      name = locker.label;
      url = locker.mirror;
      ref = "refs/eka/atoms/${locker.label}/${locker.version}";
    };
in
lockexpr ./. lockstr
