# {{crate}}

`truck` keeps local clones of every repository in a GitHub org close at hand.
`truck sync <org>` asks the GitHub CLI which repositories exist and clones the
ones whose directory is missing; `truck update <org>` fetches every clone that
already exists. Directories are named after the repositories, so a mixed org of
services, libraries and archives stays navigable without a second index.

The obvious alternative, a shell loop around `gh repo list`, forgets that
existing directories are precious: a partially cloned or hand-made directory
would be overwritten or fail every time. `truck` treats presence as ownership,
skips archived repositories unless asked, and removes only the partial clone it
created when a clone fails.

## Installation

```
nix run github:sshine/truck-cli -- sync terranix
nix profile install github:sshine/truck-cli
cargo install --path crates/truck
```

Prebuilt static binaries for x86_64 and aarch64 Linux, plus macOS aarch64,
are attached to [the releases][releases].

### NixOS

The flake exports an overlay, so a system configuration can take truck as an
ordinary attribute of `pkgs`:

```nix
{
  inputs = {
    nixpkgs.url = "https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.zst";

    truck-cli = {
      url = "github:sshine/truck-cli";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, truck-cli, ... }:
    {
      nixosConfigurations.example = nixpkgs.lib.nixosSystem {
        modules = [
          (
            { pkgs, ... }:
            {
              nixpkgs.overlays = [ truck-cli.overlays.default ];
              environment.systemPackages = [ pkgs.truck ];
            }
          )
        ];
      };
    };
}
```

The overlay builds against your nixpkgs rather than this flake's, so
`overrideAttrs` and cross-compilation work as they do for anything else in the
set. Where none of that matters, `truck-cli.packages.${system}.default` is the
same package without the overlay.

## CLI

{{readme}}

## License

MIT OR Apache-2.0.

[releases]: https://github.com/sshine/truck-cli/releases
