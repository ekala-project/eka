dep:
if dep.type == "atom" then
  builtins.hashString "sha256" (dep.id + dep.version)
else
  dep.rev or dep.hash
