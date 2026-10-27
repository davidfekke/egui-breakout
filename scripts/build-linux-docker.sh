#!/usr/bin/env bash
# Build a Linux release binary in a Debian container, so the only host
# requirement is Docker (works from macOS, Windows or Linux, any CPU).
#
#   ./scripts/build-linux-docker.sh amd64   # -> x86_64-unknown-linux-gnu
#   ./scripts/build-linux-docker.sh arm64   # -> aarch64-unknown-linux-gnu
#
# Output: target/<triple>/release/egui-breakout
#
# The binary links the container's glibc (Debian 12 = glibc 2.36); this build
# needs at most GLIBC_2.35, so it runs on Debian 12+ and Ubuntu 22.04+.
# Runtime packages needed on the target machine:
# libasound2, libvulkan1 (or a working GL/EGL stack), libxkbcommon0 and either
# libx11-6 + libxi6 or libwayland-client0.
set -euo pipefail

arch="${1:-$(uname -m)}"
case "$arch" in
  amd64 | x86_64) platform=linux/amd64; target=x86_64-unknown-linux-gnu ;;
  arm64 | aarch64) platform=linux/arm64; target=aarch64-unknown-linux-gnu ;;
  *)
    echo "usage: $(basename "$0") [amd64|arm64]" >&2
    exit 2
    ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"

docker run --rm \
  --platform "$platform" \
  -v "$root":/src \
  -v "brick-break-cargo-$arch":/usr/local/cargo/registry \
  -w /src \
  rust:1.99-bookworm \
  bash -c "
    apt-get update -qq &&
    apt-get install -y -qq --no-install-recommends \
      libasound2-dev libxkbcommon-dev libwayland-dev libx11-dev &&
    cargo build --release --locked --target $target
  "

echo
echo "built target/$target/release/egui-breakout"
