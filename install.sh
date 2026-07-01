#!/bin/sh
# note installer/updater — picks the best available method for your system,
# in order:
#   1. Homebrew (macOS/Linux)   2. .deb/.rpm (Debian/Ubuntu · Fedora/RHEL)
#   3. mise (ubi backend)       4. cargo (build from source)
#   5. prebuilt binary tarball from the GitHub release
#
# Re-running upgrades an existing install to the latest release (every branch
# installs-or-updates in place), so the same one-liner covers both:
#   curl -fsSL https://raw.githubusercontent.com/LLawli/note/master/install.sh | sh
set -eu

REPO="LLawli/note"
TAP="LLawli/tap"

have() { command -v "$1" >/dev/null 2>&1; }
say()  { printf '==> %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

have curl || die "curl is required"

latest_tag() {
  resp="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest")" \
    || die "cannot reach the GitHub API"
  # Isolate the tag_name field first: the API may return minified JSON on one
  # line, where a blind `cut -f4` would grab the first string value (`url`).
  printf '%s\n' "$resp" | grep -o '"tag_name"[^,]*' | head -1 | cut -d'"' -f4
}

# Rust target triple for the prebuilt tarballs (empty = unsupported here).
triple() {
  case "$(uname -s)/$(uname -m)" in
    Linux/x86_64)                echo x86_64-unknown-linux-gnu ;;
    Linux/aarch64 | Linux/arm64) echo aarch64-unknown-linux-gnu ;;
    Darwin/arm64 | Darwin/aarch64) echo aarch64-apple-darwin ;;
    *) echo "" ;;
  esac
}

os="$(uname -s)"
arch="$(uname -m)"

# 1. Homebrew (any OS that has it). `upgrade` covers an existing install; if it
# isn't installed yet upgrade fails and we fall through to install.
if have brew; then
  say "Homebrew"
  brew upgrade "$TAP/note" 2>/dev/null && exit 0
  exec brew install "$TAP/note"
fi

# 2. Linux distribution packages.
if [ "$os" = "Linux" ] && [ -r /etc/os-release ]; then
  . /etc/os-release
  tag="$(latest_tag)"; ver="${tag#v}"
  case "$arch" in
    x86_64)         deb_arch=amd64; rpm_arch=x86_64 ;;
    aarch64 | arm64) deb_arch=arm64; rpm_arch=aarch64 ;;
    *)              deb_arch=""; rpm_arch="" ;;
  esac
  case " ${ID:-} ${ID_LIKE:-} " in
    *" debian "* | *" ubuntu "*)
      [ -n "$deb_arch" ] || die "no .deb for $arch"
      url="https://github.com/$REPO/releases/download/$tag/note_${ver}_${deb_arch}.deb"
      say ".deb -> $url"
      tmp="$(mktemp)"; curl -fsSL "$url" -o "$tmp"; sudo dpkg -i "$tmp"; rm -f "$tmp"
      exit 0 ;;
    *" fedora "* | *" rhel "* | *" centos "*)
      [ -n "$rpm_arch" ] || die "no .rpm for $arch"
      url="https://github.com/$REPO/releases/download/$tag/note-${ver}-1.${rpm_arch}.rpm"
      say ".rpm -> $url"
      # -U installs or upgrades in place (rpm -i would refuse an upgrade).
      sudo rpm -U "$url" 2>/dev/null || sudo dnf install -y "$url"
      exit 0 ;;
  esac
fi

# 3. mise (downloads the prebuilt binary from the release via the github backend).
if have mise; then
  say "mise (github:$REPO)"
  exec mise use -g "github:$REPO"
fi

# 4. cargo (build from source). --force reinstalls over an older version.
if have cargo; then
  say "cargo install"
  exec cargo install --force --git "https://github.com/$REPO" note-cli
fi

# 5. Prebuilt binary tarball.
t="$(triple)"
[ -n "$t" ] || die "no prebuilt binary for $os/$arch — install cargo (or mise) and re-run"
tag="$(latest_tag)"
url="https://github.com/$REPO/releases/download/$tag/note-$tag-$t.tar.gz"
bindir="${HOME}/.local/bin"
mkdir -p "$bindir"
say "binary -> $url into $bindir"
tmp="$(mktemp -d)"
curl -fsSL "$url" | tar -xz -C "$tmp"
install "$tmp"/note-*/note "$bindir/note"
rm -rf "$tmp"
say "installed $bindir/note — make sure $bindir is on your PATH"
