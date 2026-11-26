# The one place crate2nix meets this flake's pkgs.
#
# Committed mode only: this reads ../Cargo.nix. With IFD, nix/packages.nix calls
# appliedCargoNix instead and this file is omitted.
#
# `_package.nix` and the overlay both call this, so they cannot resolve different
# store paths for the same dependency. Taking `pkgs` as an argument rather than
# closing over this flake's own is what lets the overlay work.
{ lib, pkgs, ... }:
import ../Cargo.nix {
  inherit pkgs;

  buildRustCrateForPkgs =
    p:
    let
      base = p.buildRustCrate.override (
        {
          # nixpkgs' defaultCrateOverrides already covers the usual *-sys crates
          # (openssl, pkg-config, ...). Add only what it misses, for example:
          #
          #   defaultCrateOverrides = p.defaultCrateOverrides // {
          #     example-sys = _attrs: {
          #       nativeBuildInputs = [ p.pkg-config ];
          #       buildInputs = [ p.openssl ];
          #     };
          #   };
          defaultCrateOverrides = p.defaultCrateOverrides;
        }
        # rust-overlay's toolchain where there is one, nixpkgs' rustc where there
        # is not. A consumer's nixpkgs need not carry `rust-bin`, and a project's
        # pinned compiler is not a thing to force on them.
        // lib.optionalAttrs (p ? rust-bin) (
          let
            toolchain = p.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml;
          in
          {
            rustc = toolchain;
            cargo = toolchain;
          }
        )
      );
    in
    crate: base crate;
}
