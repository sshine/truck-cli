# Cross-compilation for the release artifacts.
#
# Include this module only when aarch64-linux is cross-built from an x86_64 runner. If
# the release builds aarch64 on a native arm runner (ubuntu-24.04-arm), delete the file:
# nothing needs a foreign linker, and the devshell stays smaller.
#
# The body is guarded to x86_64-linux because import-tree imports every module on every
# configured system. Without the guard, a flake that also lists aarch64-darwin would put
# a Linux cross toolchain into the Darwin devshell.
#
# Building through cargo with rust-overlay's target support, rather than through
# pkgsCross.*.rustPlatform, keeps this to seconds: every dependency is pure Rust, so the
# only thing missing for a foreign target is a linker.
#
# If a dependency compiles C (ring through rustls is the usual one), a linker is not
# enough: the cc crate needs a C compiler for each musl target, exported as
# CC_x86_64_unknown_linux_musl and CC_aarch64_unknown_linux_musl. Underscores, because
# devshell exports these and a dashed name is not a shell identifier.
{ ... }:
{
  perSystem =
    {
      system,
      pkgs,
      lib,
      ...
    }:
    let
      # rustc finds musl's crt objects in its own `self-contained` directory and can
      # link x86_64 unaided; only a foreign architecture needs a cross linker.
      aarch64-cc = pkgs.pkgsCross.aarch64-multiplatform-musl.stdenv.cc;
      aarch64-linker = "${aarch64-cc}/bin/aarch64-unknown-linux-musl-cc";
    in
    lib.optionalAttrs (system == "x86_64-linux") {
      devshells.default = {
        packages = [ aarch64-cc ];

        env = [
          {
            name = "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER";
            value = aarch64-linker;
          }
        ];
      };

      # Exposed so a workflow or a debugging session can resolve the linker without
      # entering the devshell.
      packages.aarch64-musl-cc = aarch64-cc;
    };
}
