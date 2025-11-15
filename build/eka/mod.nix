let
  inherit (deps.from.eka) dev;
  inherit (dev) pkgs;
in
{
  Eka = mod.crates.workspaceMembers.eka.build;
  Crates = ext.cargo-lock {
    inherit (dev) pkgs;
    buildRustCrateForPkgs =
      pkgs:
      pkgs.buildRustCrate.override {
        rustc = dev.toolchain;
        cargo = dev.toolchain;
      };
    defaultCrateOverrides =
      pkgs.defaultCrateOverrides
      // {
        atom = self: {
          # FIXME: no choice but to hardcode these for now. The real solution will be to
          # split up the atom crate and use some of its functionality in the proc macro
          # to compute these from the local repository without having to contact the
          # network.
          EKA_ROOT_COMMIT_HASH = ext.locker.set;
          EKA_ORIGIN_URL = ext.locker.mirror;
          EKA_LOCK_REV = ext.locker.rev;
          EKA_LOCK_IMPORT = ext.fetch + "/import.nix";
        };
      }
      // builtins.listToAttrs (
        map
          (name: {
            inherit name;
            value = self: {
              buildInputs = [ pkgs.protobuf ];
              PROTO_ROOT = dev.protos;
              SNIX_BUILD_SANDBOX_SHELL = "/bin/sh";

            };
          })
          [
            "snix-castore"
            "snix-build"
            "snix-store"
          ]
      );
  };
}
