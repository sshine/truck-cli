# Prebuilt release binaries, for consumers who want truck without a Rust toolchain
# entering their build closure.
#
# The pinned version deliberately trails Cargo.toml: it names the last tag whose
# artifacts are published, not whatever the working tree happens to claim. Bump it
# once a tag's Release workflow has finished; `just release-hashes <tag>` prints the
# values.
#
# The hashes are of the tarballs the release publishes, so they can be checked against
# the .sha256 assets rather than taken on faith. Systems absent from `hashes` do not get
# the package.
#
# The release assets are public, so this package needs no GitHub credentials.
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
      release = {
        version = "0.2.0";
        hashes = {
          "x86_64-linux" = "sha256-+k7kYxvnW2IgJVgR3PHm7aKl2kMxfQXBt10t0pe8TLg=";
          "aarch64-linux" = "sha256-BFdmvuFabpU62CoaMtl8wi/VVsa3PxuJ7xlAHO1vqsg=";
          "aarch64-darwin" = "sha256-evt7B6DDVKtUHChK5Noc9htYo8X5TbDa/6RKxiC8WIs=";
        };
      };

      triple = {
        "x86_64-linux" = "x86_64-unknown-linux-musl";
        "aarch64-linux" = "aarch64-unknown-linux-musl";
        "aarch64-darwin" = "aarch64-apple-darwin";
      };

      tag = "v${release.version}";

      tarball = pkgs.fetchurl {
        url = "https://github.com/sshine/truck-cli/releases/download/${tag}/truck-${tag}-${triple.${system}}.tar.gz";
        hash = release.hashes.${system};
      };
    in
    lib.optionalAttrs (release.hashes ? ${system}) {
      packages.truck-bin =
        pkgs.runCommand "truck-bin-${release.version}"
          {
            nativeBuildInputs = [ pkgs.makeWrapper ];

            meta = {
              description = "Clone and fetch every repository in a GitHub org (prebuilt)";
              mainProgram = "truck";
              license = with lib.licenses; [
                mit
                asl20
              ];
              platforms = lib.attrNames release.hashes;
              sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
            };
          }
          ''
            tar -xzf ${tarball}
            install -Dm755 truck $out/bin/truck

            # The binary shells out to gh and git, so the wrapper puts the tools it
            # calls on PATH; without it a prebuilt truck cannot list or clone.
            wrapProgram $out/bin/truck \
              --prefix PATH : ${
                lib.makeBinPath [
                  pkgs.gh
                  pkgs.git
                ]
              }
          '';
    };
}
