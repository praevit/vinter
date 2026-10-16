#!/usr/bin/env bash
# Builds Vinter Wine: upstream Wine for macOS (x86_64, with Wine's new WoW64
# for 32-bit programs) plus the patches in wine/patches/<version>, packaged
# with the libraries Wine loads at run time.
#
# Usage: wine/build.sh <wine-version> <build-number> [out-dir]
#   e.g. wine/build.sh 11.0 1 out
#
# Built without debug info (-O2 only), which halves the size.
# Runs on an Intel Mac with Homebrew (CI: macos-15-intel). The libraries come
# from Homebrew bottles built for macOS 15, so the result needs macOS 15+.
set -euo pipefail

version=$1
build=$2
out=$(mkdir -p "${3:-out}" && cd "${3:-out}" && pwd)
here=$(cd "$(dirname "$0")" && pwd)
name="vinter-wine-$version-$build"
work=$(mktemp -d)
prefix="$work/$name"

# Wine releases live in 11.0/ for stable and 11.x/ for development versions.
case $version in
  *.0) series=$version ;;
  *) series="${version%%.*}.x" ;;
esac

echo "::group::Dependencies"
brew install --quiet bison flex mingw-w64 pkgconf freetype gnutls molten-vk sdl2 dylibbundler
brew_prefix=$(brew --prefix)
export PATH="$(brew --prefix bison)/bin:$(brew --prefix flex)/bin:$PATH"
export MACOSX_DEPLOYMENT_TARGET=15.0
echo "::endgroup::"

echo "::group::Source"
curl -fsSL -o "$work/wine.tar.xz" "https://dl.winehq.org/wine/source/$series/wine-$version.tar.xz"
tar -xf "$work/wine.tar.xz" -C "$work"
src="$work/wine-$version"
for patch in "$here/patches/$version"/*.patch; do
  [ -e "$patch" ] || continue
  echo "Applying $(basename "$patch")"
  patch -d "$src" -p1 < "$patch"
done
echo "::endgroup::"

echo "::group::Configure"
mkdir -p "$work/build"
cd "$work/build"
"$src/configure" \
  --prefix="$prefix" \
  --enable-archs=i386,x86_64 \
  --disable-tests \
  --without-x \
  --without-gstreamer \
  --with-vulkan \
  CFLAGS="-O2" \
  CROSSCFLAGS="-O2" \
  CPPFLAGS="-I$brew_prefix/include" \
  LDFLAGS="-L$brew_prefix/lib"
echo "::endgroup::"

echo "::group::Build"
make -j"$(sysctl -n hw.ncpu)"
make install-lib
echo "::endgroup::"

echo "::group::Bundle libraries"
# Wine dlopen()s these by file name; its unix modules look in
# @loader_path/../../ (that is, $prefix/lib) first.
lib="$prefix/lib"
for dylib in \
  "$(brew --prefix freetype)/lib/libfreetype.6.dylib" \
  "$(brew --prefix gnutls)/lib/libgnutls.30.dylib" \
  "$(brew --prefix molten-vk)/lib/libMoltenVK.dylib" \
  "$(brew --prefix sdl2)/lib/libSDL2-2.0.0.dylib"; do
  cp "$dylib" "$lib/"
  chmod u+w "$lib/$(basename "$dylib")"
done
for dylib in "$lib"/*.dylib; do
  install_name_tool -id "@rpath/$(basename "$dylib")" "$dylib"
  dylibbundler --overwrite-files --bundle-deps --no-codesign \
    --fix-file "$dylib" --dest-dir "$lib" --install-path @loader_path/ >/dev/null
done
# dylibbundler and install_name_tool invalidate signatures; re-sign ad hoc.
find "$prefix" -type f \( -name "*.dylib" -o -name "*.so" -o -perm -u+x \) -print0 |
  xargs -0 -n 50 codesign --force --sign - 2>/dev/null || true
for so in "$prefix"/lib/wine/x86_64-unix/ntdll.so; do
  otool -l "$so" | grep -q "@loader_path/../../" || install_name_tool -add_rpath @loader_path/../../ "$so"
done
echo "::endgroup::"

echo "::group::Package"
mkdir -p "$prefix/share/vinter"
cp "$src/COPYING.LIB" "$prefix/LICENSE-wine.txt"
cp -R "$here/patches/$version" "$prefix/share/vinter/patches" 2>/dev/null || true
cat > "$prefix/share/vinter/SOURCE.txt" <<SRC
Vinter Wine $version-$build
Wine source: https://dl.winehq.org/wine/source/$series/wine-$version.tar.xz
Changes: the patches in share/vinter/patches (also at
https://github.com/praevit/vinter/tree/main/wine/patches/$version).
Build script: https://github.com/praevit/vinter/blob/main/wine/build.sh
Wine is licensed under the GNU LGPL, version 2.1 or later (LICENSE-wine.txt).
SRC
"$prefix/bin/wine" --version
archive="$out/$name-macos-x86_64.tar.xz"
tar -C "$work" -cJf "$archive" "$name"
shasum -a 256 "$archive" | tee "$archive.sha256"
echo "::endgroup::"
