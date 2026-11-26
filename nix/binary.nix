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
# This repository is private, so anonymous `fetchurl` cannot download its release
# assets; authenticated consumers can use `gh release download` or the source build.
# The package stays here so the release is usable once the repository is public.
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
        version = "0.1.0";
        hashes = {
          "x86_64-linux" = "sha256-PNe/ny7EyhTkM5EMReZmTrh9sSlkSVLqXzNDSnEtQCU=";
          "aarch64-linux" = "sha256-7ZKoupxuEQZbFQmfASj+FjsMr9XuzeAa0G0k5OXAUE4=";
          "aarch64-darwin" = "sha256-QDzmpczGjAPSgZKOENoJJ894LV2Pa1BrWvfYUNzdXV8=";
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
