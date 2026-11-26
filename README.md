# truck

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

The repository is private, so `nix run` needs GitHub credentials. Prebuilt
binaries are attached to [the releases][releases]; download them with
`gh release download` while the repository stays private.

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

Clone and fetch every repository in a GitHub org.

`truck sync <org>` lists the org with the authenticated GitHub CLI and clones
every repository whose directory is missing. `truck update <org>` runs
`git fetch --all --prune` in every clone that already exists. The org name
doubles as the directory below the base directory.

### Usage

```console
$ truck sync terranix
cloning terranix/terranix
truck: sync terranix: 1 cloned, 2 skipped, 0 failed in /Users/ssi/Projects/terranix

$ truck sync --include-archived nix-tools
...

$ truck update terranix
fetching terranix/terranix
truck: update terranix: 3 fetched, 0 skipped, 0 failed in /Users/ssi/Projects/terranix

$ truck --base-dir ~/src sync nix-tools
```

### Options

`--base-dir <DIR>` is the only configurable path. It defaults to
`~/Projects`, and `sync` creates `<DIR>/<org>` when it is missing. Archived
repositories are skipped unless `sync` is given `--include-archived`.

Directories that already exist are never touched by `sync`, even when they
are not clones. `update` only looks at directories that are already there,
so archived repositories that were cloned explicitly keep working.

### Exit codes

`0` when every repository succeeded, `1` when one or more clones or fetches
failed, and `2` when the arguments did not parse.

## License

MIT OR Apache-2.0.

[releases]: https://github.com/sshine/truck-cli/releases
