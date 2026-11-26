{ inputs, ... }:
{
  imports = [ inputs.devshell.flakeModule ];
  perSystem =
    {
      config,
      lib,
      pkgs,
      system,
      ...
    }:
    let
      rust-toolchain = pkgs.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml;
    in
    {
      devshells.default = {
        packages = [
          rust-toolchain
          config.treefmt.build.wrapper
          # The same mdformat treefmt runs, so `just readme` normalises its output
          # exactly the way the formatter would.
          config.treefmt.build.programs.mdformat
          config.packages.cargo-readme
          pkgs.cargo-watch
          pkgs.cargo-insta
          pkgs.deadnix
          pkgs.diffutils
          pkgs.git
          pkgs.jq
          pkgs.just
          pkgs.libiconv
          pkgs.stdenv.cc
          # Regenerates Cargo.nix. Taken from this flake so the generator and the
          # committed output move together. Drop it when Cargo.nix is not
          # committed, together with config.packages.cargo-nix.
          inputs.crate2nix.packages.${system}.default
          config.packages.cargo-nix
        ]
        # The hooks phase adds config.hk-nix.package and the startup entry below.
        # `config ? hk-nix` keeps this module evaluating until that input and its
        # nix/hooks.nix module exist, and then installs the hooks without an edit.
        ++ lib.optionals (config ? hk-nix) [ config.hk-nix.package ];

        env = [
          {
            name = "RUST_BACKTRACE";
            value = "1";
          }
          # rustc links macOS binaries with -liconv; the Nix SDK does not put
          # libiconv on the default search path the way the system SDK does.
          {
            name = "LIBRARY_PATH";
            value = "${pkgs.libiconv}/lib";
          }
          {
            name = "NIX_LDFLAGS";
            value = "-L${pkgs.libiconv}/lib";
          }
        ];

        devshell.motd = "";

        devshell.startup = lib.optionalAttrs (config ? hk-nix) {
          hk.text = config.hk-nix.shellHook;
        };
      };
    };
}
