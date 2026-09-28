#!/usr/bin/env bash
# Release helpers shared by .github/workflows/release.yml and local dry runs.
#
#   release.sh version              print the workspace version from Cargo.toml
#   release.sh check-tag vX.Y.Z     fail unless the tag matches that version
#   release.sh notes X.Y.Z          print the CHANGELOG section of that version
#   release.sh package TARGET DIR   archive target/TARGET/release/pktctl with the docs into DIR
#   release.sh publish-crates [--dry-run]
#                                   publish ptmp, pktfile and pktctl at the workspace version,
#                                   skipping those crates.io already has
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

version() {
    sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$root/Cargo.toml"
}

check_tag() {
    local tag="$1" expected
    expected="v$(version)"
    if [[ "$tag" != "$expected" ]]; then
        echo "tag $tag does not match the workspace version $expected" >&2
        exit 1
    fi
}

notes() {
    local wanted="$1" body
    body="$(awk -v wanted="$wanted" '
        /^## / {
            if (found) exit
            heading = $2
            if (heading == wanted) { found = 1; next }
        }
        found { print }
    ' "$root/CHANGELOG.md")"
    if [[ -z "${body//[[:space:]]/}" ]]; then
        echo "CHANGELOG.md has no section for $wanted" >&2
        exit 1
    fi
    printf '%s\n' "$body"
}

package() {
    local target="$1" out="$2" name binary ext=""
    name="pktctl-$(version)-$target"
    [[ "$target" == *windows* ]] && ext=".exe"
    binary="$root/target/$target/release/pktctl$ext"
    [[ -f "$binary" ]] || { echo "no binary at $binary; build it first" >&2; exit 1; }
    local stage
    stage="$(mktemp -d)"
    mkdir -p "$stage/$name" "$out"
    cp "$binary" "$stage/$name/"
    cp "$root/README.md" "$root/LICENSE" "$root/CHANGELOG.md" "$stage/$name/"
    if [[ "$target" == *windows* ]]; then
        (cd "$stage" && 7z a -tzip -bso0 "$out/$name.zip" "$name")
        echo "$out/$name.zip"
    else
        tar -C "$stage" -czf "$out/$name.tar.gz" "$name"
        echo "$out/$name.tar.gz"
    fi
    rm -rf "$stage"
}

published_on_crates_io() {
    local crate="$1" version="$2" code
    code="$(curl -s -o /dev/null -w '%{http_code}' \
        -A 'pktctl-release (https://github.com/TantiE100/pktctl)' \
        "https://crates.io/api/v1/crates/$crate/$version")"
    [[ "$code" == 200 ]]
}

publish_crates() {
    local version pending=() crate
    version="$(version)"
    for crate in ptmp pktfile pktctl; do
        if published_on_crates_io "$crate" "$version"; then
            echo "$crate $version is already on crates.io"
        else
            pending+=(-p "$crate")
        fi
    done
    if ((${#pending[@]} == 0)); then
        return
    fi
    if [[ "${1:-}" == "--dry-run" ]]; then
        (cd "$root" && cargo publish --workspace --locked --dry-run --allow-dirty)
    else
        (cd "$root" && cargo publish --locked "${pending[@]}")
    fi
}

case "${1:-}" in
    version) version ;;
    check-tag) check_tag "$2" ;;
    notes) notes "$2" ;;
    package)
        mkdir -p "$3"
        package "$2" "$(cd "$3" && pwd)"
        ;;
    publish-crates) publish_crates "${2:-}" ;;
    *) sed -n '2,11p' "$0" >&2; exit 2 ;;
esac
