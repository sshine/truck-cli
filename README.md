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

Clone and fetch every repository in a GitHub org.

`truck sync <org>` lists the org with the authenticated GitHub CLI, shows a
preflight summary and waits for approval before cloning every repository
whose directory is missing. `truck update <org>` runs
`git fetch --all --prune` in every clone that already exists. The org name
doubles as the directory below the base directory.

### Usage

```console
$ truck sync terranix
truck: terranix: 12 repositories, 4 archived (skipped)
truck: 8 new (approx. 1.2 MiB), 0 existing
Clone 8 repositories? [y/N] y
truck: logging clone output to /Users/ssi/.local/state/truck/logs/terranix-1764.log
[########################] 8/8 cloned terranix/terranix
truck: sync terranix: 8 cloned, 0 skipped, 0 failed in /Users/ssi/Projects/terranix

$ truck sync --include-archived --yes nix-tools
...

$ truck update terranix
fetching terranix/terranix
truck: update terranix: 3 fetched, 0 skipped, 0 failed in /Users/ssi/Projects/terranix

$ truck --base-dir ~/src sync nix-tools
```

### Options

`--base-dir <DIR>` is the only configurable path. It defaults to
`~/Projects`, and `sync` creates `<DIR>/<org>` once cloning is approved.
Archived repositories are skipped unless `sync` is given
`--include-archived`. `--yes` skips the approval prompt for non-interactive
use.

Directories that already exist are never touched by `sync`, even when they
are not clones. `update` only looks at directories that are already there,
so archived repositories that were cloned explicitly keep working.

### Logs and progress

Clone output no longer goes to the terminal. Each run appends it to
`$XDG_STATE_HOME/truck/logs/<org>-<timestamp>.log`, defaulting to
`~/.local/state/truck/logs/`, and the path is printed before cloning
starts. The terminal shows an ANSI progress line with the completed count,
or plain status lines when stderr is not a terminal.

### Exit codes

`0` when every repository succeeded or there was nothing to do, `1` when a
clone or fetch failed or the preflight was declined, and `2` when the
arguments did not parse.

## License

MIT OR Apache-2.0.

[releases]: https://github.com/sshine/truck-cli/releases
