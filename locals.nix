root:
let
  builder =
    path:
    let
      ekala = path + "/ekala.toml";
      hasSet =
        builtins.readFileType path == "directory" && builtins.isPath ekala && builtins.pathExists ekala;
      toml = builtins.fromTOML (builtins.readFile ekala);
      locals = builtins.listToAttrs (
        map (
          rel:
          let
            toml_path = path + "/${rel}/atom.toml";
            atom = builtins.fromTOML (builtins.readFile toml_path);
          in
          {
            name = atom.package.label;
            value = dirOf toml_path;
          }
        ) toml.set.packages
      );
    in
    if path == /. || path == "/" then
      { }
    else if hasSet then
      locals
    else
      builder (dirOf path);
in
builder root
