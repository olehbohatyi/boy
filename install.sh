#!/bin/sh
# Install boy: curl -fsSL https://raw.githubusercontent.com/olehbohatyi/boy/main/install.sh | sh
#
# Downloads the latest boy release by direct link (no GitHub API), checks it against
# SHA256SUMS, and lets that boy install itself into the boy home with `boy i boy`.
# The boy home is $BOY_HOME, else ~/.boy. At the end it offers to add <home>/bins to
# PATH; a shell startup file is changed only when you answer yes.

set -eu

repo="olehbohatyi/boy"
home="${BOY_HOME:-$HOME/.boy}"

say() { printf '%s\n' "$*" >&2; }
fail() {
    say "boy install: $*"
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || fail "\`$1\` is needed but not found"
}

target() {
    [ "$(uname -s)" = Darwin ] || fail "only macOS is supported for now (this is $(uname -s))"
    arch=$(uname -m)
    # A shell running under Rosetta reports x86_64 on Apple Silicon.
    if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
        arch=arm64
    fi
    case "$arch" in
        x86_64) echo x86_64-apple-darwin ;;
        arm64 | aarch64) echo aarch64-apple-darwin ;;
        *) fail "unsupported cpu: $arch" ;;
    esac
}

# The latest tag, from the redirect of releases/latest.
latest_tag() {
    url=$(curl -fsS -o /dev/null -w '%{redirect_url}' "https://github.com/$repo/releases/latest") ||
        fail "cannot reach github.com; check the network"
    tag=${url##*/tag/}
    [ -n "$url" ] && [ "$tag" != "$url" ] || fail "no boy release found at github.com/$repo/releases"
    echo "$tag"
}

# Offer to add the bins folder to PATH in the shell's startup file.
offer_path() {
    bins="$home/bins"
    case ":$PATH:" in
        *":$bins:"*) return ;;
    esac
    case "$bins" in
        "$HOME"/*) line="export PATH=\"\$HOME/${bins#"$HOME"/}:\$PATH\"" ;;
        *) line="export PATH=\"$bins:\$PATH\"" ;;
    esac
    case "${SHELL:-}" in
        */zsh) rc="$HOME/.zshrc" ;;
        */bash) rc="$HOME/.bash_profile" ;;
        *) rc="" ;;
    esac

    if [ -n "$rc" ] && [ -f "$rc" ] && grep -qxF "$line" "$rc"; then
        say "$bins is added to PATH in $rc; open a new terminal to use boy"
        return
    fi

    answer=n
    # The script itself is usually on stdin (curl | sh), so ask on the terminal.
    if [ -n "$rc" ] && { : </dev/tty; } 2>/dev/null; then
        printf 'add %s to PATH in %s? [Y/n] ' "$bins" "$rc" >&2
        read -r answer </dev/tty || answer=n
        [ -n "$answer" ] || answer=y
    fi
    case "$answer" in
        y | Y | yes | YES | Yes)
            printf '\n# boy\n%s\n' "$line" >>"$rc"
            say "added to $rc; open a new terminal to use boy"
            ;;
        *)
            say "to use boy, add this line to your shell startup file:"
            say "  $line"
            ;;
    esac
}

main() {
    need curl
    need tar
    need shasum
    need uname

    if [ -e "$home/bins/boy" ]; then
        say "boy is already installed in $home; update it with \`boy u boy\`"
        exit 0
    fi

    target=$(target)
    tag=$(latest_tag)
    version=${tag#v}
    file="boy-$version-$target.tar.gz"
    base="https://github.com/$repo/releases/download/$tag"

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT INT TERM

    say "downloading boy $version ($target)"
    curl -fsSL -o "$tmp/$file" "$base/$file" || fail "cannot download $base/$file"
    curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" || fail "cannot download $base/SHA256SUMS"
    (cd "$tmp" && awk -v f="$file" '$2 == f' SHA256SUMS | grep . | shasum -a 256 -c -s) ||
        fail "checksum of $file does not match SHA256SUMS; try again later"
    tar -xzf "$tmp/$file" -C "$tmp" boy || fail "cannot unpack $file"

    # boy installs itself, so it is recorded in lock.toml like any other package.
    BOY_HOME="$home" "$tmp/boy" i boy || fail "\`boy i boy\` failed; see the message above"
    "$home/bins/boy" >/dev/null || fail "the installed $home/bins/boy does not run"
    say "installed boy $version in $home"

    offer_path
}

main "$@"
