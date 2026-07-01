#!/bin/sh
# note installer/updater — picks the best available method for your system,
# in order:
#   1. Homebrew (macOS/Linux)   2. .deb/.rpm (Debian/Ubuntu · Fedora/RHEL)
#   3. mise (github backend)    4. cargo (build from source)
#   5. prebuilt binary tarball from the GitHub release
#
# There is no prebuilt binary for Intel (x86_64) macOS, so there `note` is built
# from source (Homebrew's formula compiles too); every other target is prebuilt.
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
  resp="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest")" || return 1
  # Isolate the tag_name field first: the API may return minified JSON on one
  # line, where a blind `cut -f4` would grab the first string value (`url`).
  printf '%s\n' "$resp" | grep -o '"tag_name"[^,]*' | head -1 | cut -d'"' -f4
}

# Resolve the latest release tag into $tag, or abort. (Called in the main shell,
# so `die` here exits the script — unlike a `die` inside `$(...)`.)
resolve_tag() {
  tag="$(latest_tag)" || die "cannot reach the GitHub API"
  [ -n "$tag" ] || die "could not find the latest release tag"
}

# sha256 of a file, using whichever tool is present (empty output if neither).
sha_of() {
  if have sha256sum; then sha256sum "$1" | cut -d' ' -f1
  elif have shasum; then shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# Rust target triple for the prebuilt tarballs (empty = build from source).
triple() {
  case "$(uname -s)/$(uname -m)" in
    Linux/x86_64)                  echo x86_64-unknown-linux-gnu ;;
    Linux/aarch64 | Linux/arm64)   echo aarch64-unknown-linux-gnu ;;
    Darwin/arm64 | Darwin/aarch64) echo aarch64-apple-darwin ;;
    *) echo "" ;;
  esac
}

# Build and install from a pinned release tag. --locked honors the committed
# Cargo.lock; --tag pins the release rather than compiling the default branch.
cargo_from_source() { # $1: reason for the log line
  say "cargo install ($1)"
  resolve_tag
  exec cargo install --force --locked --git "https://github.com/$REPO" --tag "$tag" note-cli
}

os="$(uname -s)"
arch="$(uname -m)"

# 1. Homebrew (any OS that has it). On Intel macOS the formula compiles from
# source; elsewhere it installs the prebuilt binary. `upgrade` covers an existing
# install; if it isn't installed yet, upgrade fails and we fall through to install.
if have brew; then
  say "Homebrew"
  brew upgrade "$TAP/note" 2>/dev/null && exit 0
  exec brew install "$TAP/note"
fi

# Intel macOS: no prebuilt binary is published, so source is the only method.
if [ "$os" = "Darwin" ] && { [ "$arch" = "x86_64" ] || [ "$arch" = "i386" ]; }; then
  have cargo && cargo_from_source "Intel macOS builds from source"
  die "Intel macOS has no prebuilt binary — install Rust (https://rustup.rs) and re-run, or use Homebrew"
fi

# 2. Linux distribution packages.
if [ "$os" = "Linux" ] && [ -r /etc/os-release ]; then
  . /etc/os-release
  resolve_tag
  ver="${tag#v}"
  case "$arch" in
    x86_64)          deb_arch=amd64; rpm_arch=x86_64 ;;
    aarch64 | arm64) deb_arch=arm64; rpm_arch=aarch64 ;;
    *)               deb_arch=""; rpm_arch="" ;;
  esac
  case " ${ID:-} ${ID_LIKE:-} " in
    *" debian "* | *" ubuntu "*)
      [ -n "$deb_arch" ] || die "no .deb for $arch"
      url="https://github.com/$REPO/releases/download/$tag/note_${ver}_${deb_arch}.deb"
      say ".deb -> $url"
      tmp="$(mktemp)"
      curl -fsSL "$url" -o "$tmp" || die "download failed: $url"
      sudo dpkg -i "$tmp"
      rm -f "$tmp"
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

# 3. mise (fetches the prebuilt binary from the release via the github backend).
if have mise; then
  say "mise (github:$REPO)"
  exec mise use -g "github:$REPO"
fi

# 4. cargo (build from source).
have cargo && cargo_from_source "from source"

# 5. Prebuilt binary tarball.
t="$(triple)"
[ -n "$t" ] || die "no prebuilt binary for $os/$arch — install cargo (or mise) and re-run"
resolve_tag
base="https://github.com/$REPO/releases/download/$tag"
url="$base/note-$tag-$t.tar.gz"
bindir="${HOME}/.local/bin"
mkdir -p "$bindir"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
say "binary -> $url into $bindir"
curl -fsSL "$url" -o "$tmp/note.tgz" || die "download failed: $url"
# Verify the co-located checksum when a sha tool is available (guards a corrupt
# or truncated download; not a substitute for a signature).
if curl -fsSL "$url.sha256" -o "$tmp/note.sha256" 2>/dev/null; then
  expected="$(cut -d' ' -f1 "$tmp/note.sha256")"
  actual="$(sha_of "$tmp/note.tgz")"
  if [ -n "$expected" ] && [ -n "$actual" ] && [ "$expected" != "$actual" ]; then
    die "checksum mismatch for note-$tag-$t.tar.gz"
  fi
fi
tar -xzf "$tmp/note.tgz" -C "$tmp"
install "$tmp"/note-*/note "$bindir/note"
say "installed $bindir/note — make sure $bindir is on your PATH"
