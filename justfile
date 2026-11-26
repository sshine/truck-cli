# Default recipe: list available commands
default:
    @just --list

# Format all code (Rust + Nix + Markdown)
fmt:
    treefmt

# Check formatting (Rust + Nix + Markdown)
fmt-check:
    treefmt --fail-on-change --no-cache

# Run clippy lints
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Cargo.nix lives at the root, outside the paths named here, so the generated file
# is not scanned. A hook that passes every staged *.nix must exclude it itself.
#
# Find dead Nix code
lint-nix:
    deadnix --fail nix/ flake.nix

# Run all tests
test:
    cargo test --all-features

# Run tests with verbose output
test-verbose:
    cargo test --all-features -- --nocapture

# Build release
build:
    cargo build --release --all-features

# Generate documentation
doc *args='':
    cargo doc --no-deps --all-features {{args}}

readme_args := "--project-root crates/truck --input crates/truck/src/main.rs --template README.tpl"

# Regenerate README.md from README.tpl and the CLI docs, then normalise it
readme:
    cargo readme {{readme_args}} | mdformat - > README.md

# Check README.md is in sync with README.tpl and the CLI docs
readme-check:
    cargo readme {{readme_args}} | mdformat - | diff - README.md

# Verify Cargo.lock is in step with the manifests
lock-check:
    cargo metadata --locked --format-version 1 > /dev/null

# Regenerate Cargo.nix from Cargo.toml and Cargo.lock
cargo-nix:
    cargo-nix

# Check Cargo.nix is in step with Cargo.toml and Cargo.lock
cargo-nix-check:
    cargo-nix --check

# Takes an explicit semver (`just bump 0.2.0`) or a release kind (`just bump
# minor`). The version has one home, [workspace.package]; members inherit it and
# internal path dependencies carry a copy only when they are published.
#
# This does not tag. Review, commit and push main, let the main CI run go green,
# and only then run the tag recipe. A tag pushed first starts a release job that
# competes with CI for the same Nix cache key, so both run cold.
#
# Move the workspace to a new version and refresh everything derived from it
bump version:
    #!/usr/bin/env bash
    set -euo pipefail

    manifest=Cargo.toml
    request="{{version}}"

    current="$(awk '
      /^\[workspace\.package\]$/ { in_package = 1; next }
      /^\[/ { in_package = 0 }
      in_package && /^version[[:space:]]*=/ {
        sub(/^[^=]*=[[:space:]]*"/, "")
        sub(/".*$/, "")
        print
        exit
      }
    ' "$manifest")"

    if [[ -z "$current" ]]; then
      echo "no version line in [workspace.package] of $manifest" >&2
      exit 1
    fi

    if [[ "$current" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+) ]]; then
      major="${BASH_REMATCH[1]}"
      minor="${BASH_REMATCH[2]}"
      patch="${BASH_REMATCH[3]}"
    else
      echo "current version is not semver: $current" >&2
      exit 1
    fi

    case "$request" in
      major) new="$((major + 1)).0.0" ;;
      minor) new="$major.$((minor + 1)).0" ;;
      patch) new="$major.$minor.$((patch + 1))" ;;
      *)
        if [[ ! "$request" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
          echo "not semver and not major/minor/patch: $request" >&2
          exit 1
        fi
        new="$request"
        ;;
    esac

    tmp="$(mktemp)"
    trap 'rm -f "$tmp"' EXIT
    awk -v version="$new" '
      /^\[workspace\.package\]$/ { in_package = 1; print; next }
      /^\[/ { in_package = 0 }
      in_package && /^version[[:space:]]*=/ && !done {
        sub(/=.*/, "= \"" version "\"")
        done = 1
      }
      { print }
    ' "$manifest" > "$tmp"

    if ! grep -qx "version = \"$new\"" "$tmp"; then
      echo "the version line in [workspace.package] was not rewritten" >&2
      exit 1
    fi
    mv "$tmp" "$manifest"
    trap - EXIT

    # A published crate has to carry the same version beside every internal `path`
    # dependency, or `cargo package` rejects it. Rewrite only lines that are both,
    # which leaves member manifests and unrelated tables alone. House style keeps
    # these inline tables on one line; a multi-line one has to be updated by hand.
    while IFS= read -r -d '' file; do
      sed -i -E '/path[[:space:]]*=/ s/version[[:space:]]*=[[:space:]]*"[^"]*"/version = "'"$new"'"/' "$file"
    done < <(find . -name Cargo.toml -not -path './target/*' -print0)

    cargo update --workspace

    if [[ -f Cargo.nix ]]; then
      just cargo-nix
    fi

    printf '\nBumped %s -> %s.\n\nNext:\n' "$current" "$new"
    printf '  1. Review:     git diff\n'
    printf '  2. Stage:      git add -A\n'
    printf '  3. Commit:     git commit -m "chore(release): v%s"\n' "$new"
    printf '  4. Push main:  git push origin main\n'
    printf '  5. Wait for the main CI run to go green.\n'
    printf '  6. Tag:        run the tag recipe with v%s\n' "$new"
    printf '\nTag only after main is green. A tag pushed earlier starts a release job\n'
    printf 'that competes with CI for the same Nix cache key.\n'

# Run CI checks locally
ci: fmt-check lint lint-nix test doc readme-check lock-check cargo-nix-check build
    @echo "All CI checks passed!"

# Clean build artifacts
clean:
    cargo clean

# ---------------------------------------------------------------------------
# Release recipes
# ---------------------------------------------------------------------------

crate := "truck"
release_repo := "sshine/truck-cli"
release_targets := "x86_64-unknown-linux-musl aarch64-unknown-linux-musl"
macos_target := "aarch64-apple-darwin"

# The CI workflow whose latest run on main must be green before tagging. Use the file
# name, not the display name; `gh run list --workflow` accepts either, but the file name
# survives a title edit. The standard layout calls it ci.yml.
ci_workflow := "ci.yml"

# The URL the `tag` recipe prints when it cannot read CI state itself.
ci_url := "https://github.com/sshine/truck-cli/actions?query=branch%3Amain"

# Assert the release tag names the version cargo would publish
check-version version:
    @pkgid="$(cargo pkgid -p {{crate}})"; crate="v${pkgid##*#}"; \
    if [ "$crate" != "{{version}}" ]; then \
        echo "tag {{version}} does not match crate version $crate" >&2; exit 1; \
    fi

# A matrix leg calls this for the one target it was given; the leg starts from an empty
# checkout, so the append is a no-op there. Locally it is what lets a second target be
# added without rebuilding the first.
#
# Build one target into dist/ without clearing it
dist-target version target: (check-version version)
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p dist
    cargo build --release --locked --target "{{target}}" -p {{crate}}
    name="{{crate}}-{{version}}-{{target}}"
    tar -czf "dist/$name.tar.gz" -C "target/{{target}}/release" {{crate}}
    ( cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256" )

# The tarball name carries the target triple because nothing inside the archive says
# which architecture it is until the loader fails. The version in the name is the tag
# verbatim, `v` included, because nix/binary.nix builds the download URL from the tag.
#
# Build the static release artifacts and their checksums into dist/
dist-linux version: (check-version version)
    #!/usr/bin/env bash
    set -euo pipefail
    rm -rf dist && mkdir -p dist
    for target in {{release_targets}}; do
        just dist-target "{{version}}" "$target"
    done
    ls -1 dist

# macOS binaries are not static: they link libSystem and the system frameworks, so the
# archive is a normal dynamic executable. Building the target on a Linux host needs the
# macOS SDK, which is not redistributable; the native runner has it already.
#
# Build the macOS aarch64 tarball on a Mac
dist-macos version: (check-version version)
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p dist
    # A no-op when rust-toolchain.toml already lists the target, which it should.
    rustup target add {{macos_target}}
    cargo build --release --locked --target "{{macos_target}}" -p {{crate}}
    name="{{crate}}-{{version}}-{{macos_target}}"
    tar -czf "dist/$name.tar.gz" -C "target/{{macos_target}}/release" {{crate}}
    # macOS ships shasum, not sha256sum; the sidecar is the same format.
    ( cd dist && shasum -a 256 "$name.tar.gz" > "$name.tar.gz.sha256" )
    ls -1 dist

# Local builds are not bit-identical to the ones CI publishes, so hashing dist/ would
# give values that never match what fetchurl downloads. This downloads the assets the
# release actually serves, which also works while the repository is private.
#
# Print the SRI hashes nix/binary.nix pins, read from the published release
release-hashes version:
    #!/usr/bin/env bash
    set -euo pipefail
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    gh release download "{{version}}" --repo {{release_repo}} --dir "$tmp"
    for target in {{release_targets}} {{macos_target}}; do
        case "$target" in
            *-apple-darwin) system="aarch64-darwin" ;;
            x86_64-*) system="x86_64-linux" ;;
            aarch64-*) system="aarch64-linux" ;;
            *) echo "no Nix system name for $target" >&2; exit 1 ;;
        esac
        name="{{crate}}-{{version}}-$target.tar.gz"
        hash="$(nix hash file --sri --type sha256 "$tmp/$name")"
        printf '"%s" = "%s";\n' "$system" "$hash"
    done

# Only right when dist/ holds the bytes the release published: download the assets
# first. A locally compiled tarball gives a hash fetchurl will reject.
#
# Print the same SRI lines from a local dist/, for an offline check
dist-hashes:
    #!/usr/bin/env bash
    set -euo pipefail
    for target in {{release_targets}} {{macos_target}}; do
        case "$target" in
            *-apple-darwin) system="aarch64-darwin" ;;
            x86_64-*) system="x86_64-linux" ;;
            aarch64-*) system="aarch64-linux" ;;
            *) echo "no Nix system name for $target" >&2; exit 1 ;;
        esac
        archive=(dist/{{crate}}-*-"$target".tar.gz)
        if [ ! -e "${archive[0]}" ]; then
            echo "no tarball for $target in dist/" >&2
            exit 1
        fi
        hash="$(nix hash file --sri --type sha256 "${archive[0]}")"
        printf '"%s" = "%s";\n' "$system" "$hash"
    done

# ---------------------------------------------------------------------------
# Tagging
# ---------------------------------------------------------------------------

# Refuses unless the tree is clean, HEAD is exactly origin/main, the tag exists neither
# locally nor on origin, and the manifest carries {{version}}. Then it requires the CI
# workflow's latest run on main to be green: a tag pushed before that starts a second job
# that competes for the same Nix cache key instead of restoring it, so both compile and
# neither saves. HEAD must be the commit that run built, not merely an ancestor of it.
#
# Tag the current commit and push the tag, which is what starts the release
tag version: (check-version version)
    #!/usr/bin/env bash
    set -euo pipefail
    tag="{{version}}"

    if [ -n "$(git status --porcelain)" ]; then
        echo "working tree is dirty; commit or stash first" >&2
        exit 1
    fi
    if [ "$(git rev-parse --abbrev-ref HEAD)" != "main" ]; then
        echo "not on main; a release tag names a commit on main" >&2
        exit 1
    fi

    git fetch --quiet origin main
    git fetch --quiet --tags origin
    if [ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]; then
        echo "HEAD is not origin/main; push main, or check out the commit main points at" >&2
        exit 1
    fi
    if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
        echo "tag $tag already exists locally" >&2
        exit 1
    fi
    if git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1; then
        echo "tag $tag already exists on origin" >&2
        exit 1
    fi

    if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
        conclusion="$(gh run list --branch main --workflow "{{ci_workflow}}" --limit 1 --json conclusion --jq '.[0].conclusion // ""')"
        if [ "$conclusion" != "success" ]; then
            echo "the latest run on main concluded '${conclusion:-none}'; wait for green:" >&2
            gh run list --branch main --limit 5
            exit 1
        fi
    elif [ "${CONFIRM:-}" != "green" ]; then
        echo "gh is not available, so main's CI state cannot be read." >&2
        echo "Check {{ci_url}} and rerun with CONFIRM=green." >&2
        exit 1
    fi

    git tag -a "$tag" -m "$tag"
    git push origin "$tag"
