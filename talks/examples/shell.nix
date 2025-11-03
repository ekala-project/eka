{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    # Hashing tools
    openssl
    coreutils

    # JSON processing
    jq

    # Text processing
    gawk

    # Git for examples
    git

    # Nix itself for drv examples
    nix

    # For presentations
    pandoc
    mdslides  # If available, otherwise we'll use the local one
  ];

  shellHook = ''
    echo "Presentation examples environment loaded!"
    echo "Available tools: openssl, jq, awk, git, nix"
    echo ""
    echo "Run examples with:"
    echo "  ./examples/01-environment-degradation.sh"
    echo "  ./examples/02-merkle-tree-demo.sh"
    echo "  ./examples/03-static-build-recipes.sh"
  '';
}