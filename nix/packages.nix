# Every binary the workspace ships, built through crate2nix.
#
# Committed-Cargo.nix form. With IFD, replace this file with the appliedCargoNix
# version in references/crate2nix.md and drop the overlay.
#
# One derivation per crate means the binaries, the checks and the devshell reach
# the same store paths for everything they share. `cargoNix.workspaceMembers` is
# the generated file's public surface; a member with no binary has nothing for
# `nix run` to run and belongs in `checks`, not `packages`.
{ ... }:
{
  # The overlay builds against the consumer's nixpkgs rather than this flake's, so
  # `overrideAttrs` and cross-compilation work as they do for anything else in the
  # set. It needs the committed Cargo.nix: IFD would make every consumer evaluate
  # and write to their store before they can build.
  flake.overlays.default = final: _prev: {
    truck = final.callPackage ./_package.nix { };
  };

  perSystem =
    { pkgs, lib, ... }:
    let
      cargoNix = pkgs.callPackage ./_cargo-nix.nix { };

      # One line per binary. Library crates are not packages.
      binaries = [ "truck" ];

      built = lib.genAttrs binaries (crate: cargoNix.workspaceMembers.${crate}.build);
    in
    rec {
      packages = built // {
        default = pkgs.callPackage ./_package.nix { };
      };

      # One check per binary, so `nix flake check` builds every shipped crate.
      checks = built;

      apps.default = {
        type = "app";
        program = lib.getExe packages.default;
      };
    };
}
