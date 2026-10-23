#!/usr/bin/env bash
# Builds an Apple Silicon runtime for Vinter's arm64-fex engine: native
# ARM64 Wine (citi94's macOS ARM64 port) with FEX-Emu as its in-process x86
# emulator (libarm64ecfex.dll for x86-64, libwow64fex.dll for 32-bit), from
# configurable git sources. Produces the runtime tarball, its sha256 and a
# runtimes.json entry. Nothing is published: a person reviews the result.
#
# Usage: wine/build-arm64.sh <build-number> [out-dir]
# Sources (environment, all optional):
#   WINE_GIT, WINE_REF        Wine tree (default citi94/wine-macos-arm64, macos-arm64-port)
#   PATCHES_GIT, PATCHES_REF  patch set with patches/{wine,fex,llvm-mingw}/
#                             (default Irz-Z/wine-macos-native, v0.1.0; set
#                             PATCHES_GIT=none to build unpatched)
#   FEX_GIT, FEX_REF          FEX tree (default FEX-Emu/FEX, FEX-2609)
#
# Build steps follow the sources' own notes: citi94's MACOS-ARM64-PORT.md
# (configure --enable-archs=aarch64 with *-w64-mingw32-clang on PATH, loader
# codesigned with JIT entitlements) and FEX's Windows CMake targets
# (arm64ecfex and wow64fex, Data/CMake/toolchain_mingw.cmake). The ARM64EC
# arch is added as Irz-Z's build does (FEX's ARM64EC emulator needs it).
# Runs on an Apple Silicon Mac with Homebrew (CI: macos-15).
#
# UNTESTED in CI: written for the owner to run and review (see DECISIONS.md, M8.15).
set -euo pipefail

build=$1
out=$(mkdir -p "${2:-out}" && cd "${2:-out}" && pwd)
work=$(mktemp -d)

WINE_GIT=${WINE_GIT:-https://github.com/citi94/wine-macos-arm64}
WINE_REF=${WINE_REF:-macos-arm64-port}
PATCHES_GIT=${PATCHES_GIT:-https://github.com/Irz-Z/wine-macos-native}
PATCHES_REF=${PATCHES_REF:-v0.1.0}
FEX_GIT=${FEX_GIT:-https://github.com/FEX-Emu/FEX}
FEX_REF=${FEX_REF:-FEX-2609}

# The toolchain Irz-Z's mingw header patch was made against. The sha256 is
# GitHub's digest for the release asset.
LLVM_MINGW=llvm-mingw-20260922-ucrt-macos-universal
LLVM_MINGW_URL=https://github.com/mstorsjo/llvm-mingw/releases/download/20260922/$LLVM_MINGW.tar.xz
LLVM_MINGW_SHA256=52e5f5a7b131021d0c39a37a38fa380a1da7885cd04bd61afd0cd4ecfb8bc1f3

echo "::group::Dependencies"
brew install --quiet bison flex pkgconf cmake ninja freetype gnutls
export PATH="$(brew --prefix bison)/bin:$(brew --prefix flex)/bin:$PATH"
export MACOSX_DEPLOYMENT_TARGET=26.5
echo "::endgroup::"

echo "::group::Sources"
git clone --quiet --depth 1 --branch "$WINE_REF" "$WINE_GIT" "$work/wine"
git clone --quiet --depth 1 --branch "$FEX_REF" --recurse-submodules --shallow-submodules "$FEX_GIT" "$work/fex"
curl -fsSL -o "$work/llvm-mingw.tar.xz" "$LLVM_MINGW_URL"
echo "$LLVM_MINGW_SHA256  $work/llvm-mingw.tar.xz" | shasum -a 256 -c -
tar -xf "$work/llvm-mingw.tar.xz" -C "$work"
mingw="$work/$LLVM_MINGW"
if [ "$PATCHES_GIT" != none ]; then
  git clone --quiet --depth 1 --branch "$PATCHES_REF" "$PATCHES_GIT" "$work/patches"
  for p in "$work"/patches/patches/llvm-mingw/*.patch; do [ -e "$p" ] && patch -d "$mingw" -p1 < "$p"; done
  for p in "$work"/patches/patches/fex/*.patch; do
    [ -e "$p" ] || continue
    # FEX's own files, or its vixl submodule (paths start with src/aarch64).
    if grep -q '^+++ b/src/aarch64/' "$p"; then patch -d "$work/fex/External/vixl" -p1 < "$p"
    else patch -d "$work/fex" -p1 < "$p"; fi
  done
  for p in "$work"/patches/patches/wine/*.patch; do [ -e "$p" ] && patch -d "$work/wine" -p1 < "$p"; done
fi
export PATH="$mingw/bin:$PATH"
echo "::endgroup::"

echo "::group::FEX"
for triple in arm64ec-w64-mingw32:arm64ecfex aarch64-w64-mingw32:wow64fex; do
  tgt=${triple%%:*}
  target=${triple##*:}
  cmake -S "$work/fex" -B "$work/fex-$tgt" -G Ninja \
    -DCMAKE_TOOLCHAIN_FILE="$work/fex/Data/CMake/toolchain_mingw.cmake" \
    -DMINGW_TRIPLE="$tgt" -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_TESTING=False -DENABLE_JEMALLOC_GLIBC_ALLOC=False -DENABLE_LTO=False
  ninja -C "$work/fex-$tgt" "$target"
done
echo "::endgroup::"

echo "::group::Wine"
mkdir -p "$work/build"
(cd "$work/build" && "$work/wine/configure" --enable-archs=aarch64,arm64ec,i386 \
  --without-x --disable-tests --prefix=/install CC=clang)
make -C "$work/build" -j"$(sysctl -n hw.ncpu)"
make -C "$work/build" install DESTDIR="$work/root"
install="$work/root/install"
cp "$work"/fex-arm64ec-w64-mingw32/Bin/libarm64ecfex.dll "$install/lib/wine/aarch64-windows/" 2>/dev/null \
  || find "$work/fex-arm64ec-w64-mingw32" -name libarm64ecfex.dll -exec cp {} "$install/lib/wine/aarch64-windows/" \;
cp "$work"/fex-aarch64-w64-mingw32/Bin/libwow64fex.dll "$install/lib/wine/aarch64-windows/" 2>/dev/null \
  || find "$work/fex-aarch64-w64-mingw32" -name libwow64fex.dll -exec cp {} "$install/lib/wine/aarch64-windows/" \;
test -f "$install/lib/wine/aarch64-windows/libarm64ecfex.dll"
test -f "$install/lib/wine/aarch64-windows/libwow64fex.dll"
# The loader needs JIT entitlements (citi94's notes); ad-hoc signed here.
cat > "$work/entitlements.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>com.apple.security.cs.allow-jit</key><true/>
  <key>com.apple.security.cs.allow-unsigned-executable-memory</key><true/>
  <key>com.apple.security.cs.disable-library-validation</key><true/>
</dict></plist>
PLIST
codesign --force --sign - --entitlements "$work/entitlements.plist" "$install/bin/wine"
"$install/bin/wine" --version
echo "::endgroup::"

echo "::group::Package"
wine_version=$(sed -n 's/^Wine version //p' "$work/wine/VERSION")
name="vinter-wine-arm64-$wine_version-$build"
mkdir -p "$install/share/vinter"
{
  echo "Wine: $WINE_GIT at $WINE_REF ($(git -C "$work/wine" rev-parse HEAD))"
  echo "FEX: $FEX_GIT at $FEX_REF ($(git -C "$work/fex" rev-parse HEAD))"
  echo "Patches: $PATCHES_GIT at $PATCHES_REF"
  echo "Toolchain: $LLVM_MINGW"
} > "$install/share/vinter/SOURCES.txt"
tar -czf "$out/$name.tar.gz" -C "$work/root" install
sha=$(shasum -a 256 "$out/$name.tar.gz" | cut -d' ' -f1)
size=$(stat -f%z "$out/$name.tar.gz")
echo "$sha  $name.tar.gz" > "$out/$name.tar.gz.sha256"
cat > "$out/manifest-entry.json" <<JSON
{
  "id": "$name-macos",
  "name": "Apple Silicon Wine $wine_version",
  "version": "$wine_version",
  "flavor": "devel",
  "os": "macos",
  "arch": "aarch64",
  "engine": "arm64-fex",
  "url": "https://github.com/praevit/vinter/releases/download/$name/$name.tar.gz",
  "sha256": "$sha",
  "size": $size,
  "format": "tar_gz",
  "wine_bin": "install/bin/wine",
  "license": "LGPL-2.1-or-later",
  "source_url": "$WINE_GIT/tree/$WINE_REF",
  "release_url": "https://github.com/praevit/vinter/releases/tag/$name",
  "graphics": ["wined3d"],
  "min_os_version": "26.5",
  "fex_version": "$FEX_REF",
  "channel": "nightly"
}
JSON
echo "::endgroup::"
echo "Built $out/$name.tar.gz ($sha)"
