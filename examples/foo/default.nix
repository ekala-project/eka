let
  locker = import (
    builtins.fetchGit {
      url = "https://github.com/ekala-project/eka";
      ref = "refs/eka/atoms/nix-lock/0.1.0";
      rev = "d4f261e4f8dd43e2285a85b2bef03c0add219657";
    }
  );
  lockstr = builtins.readFile ./atom.lock;
in
locker ./. lockstr { }
