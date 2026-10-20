#!/usr/bin/env bash
# Builds Vinter Wine: upstream Wine for macOS (x86_64, with Wine's new WoW64
# for 32-bit programs) plus the patches in wine/patches/<version>, packaged
# with the libraries Wine loads at run time.
#
# Usage: wine/build.sh <wine-version> <build-number> [out-dir]
#   e.g. wine/build.sh 11.0 1 out
#
# By default the source is the official Wine release tarball and the patches
# come from wine/patches/<wine-version>. To build another source tree
# instead, set WINE_GIT and WINE_REF (a tag), and PATCHES to the patch
# folder name under wine/patches (it may not exist: no patches), e.g.
#   WINE_GIT=https://github.com/Alien4042x/WineForge WINE_REF=0.6.0.6 \
#   PATCHES=wineforge-0.6.0.6 wine/build.sh 11.19 1 out
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
brew install --quiet bison flex mingw-w64 pkgconf freetype gnutls molten-vk sdl2 krb5 dylibbundler
brew_prefix=$(brew --prefix)
export PATH="$(brew --prefix bison)/bin:$(brew --prefix flex)/bin:$PATH"
# MIT Kerberos for Wine's Kerberos and Negotiate support (domain sign-in).
# It is keg-only; pkg-config finds it here instead of macOS's Heimdal.
krb5=$(brew --prefix krb5)
export PKG_CONFIG_PATH="$krb5/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export MACOSX_DEPLOYMENT_TARGET=15.0
echo "::endgroup::"

patches="${PATCHES:-$version}"
echo "::group::Source"
src="$work/wine-$version"
if [ -n "${WINE_GIT:-}" ]; then
  git clone --quiet --depth 1 --branch "$WINE_REF" "$WINE_GIT" "$src"
  source_desc="$WINE_GIT at $WINE_REF ($(git -C "$src" rev-parse HEAD))"
else
  curl -fsSL -o "$work/wine.tar.xz" "https://dl.winehq.org/wine/source/$series/wine-$version.tar.xz"
  tar -xf "$work/wine.tar.xz" -C "$work"
  source_desc="https://dl.winehq.org/wine/source/$series/wine-$version.tar.xz"
fi
for patch in "$here/patches/$patches"/*.patch; do
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
# Vinter's domain accounts need Kerberos; never ship a build without it.
test -e "$prefix/lib/wine/x86_64-unix/kerberos.so" || { echo "Kerberos support was not built" >&2; exit 1; }
echo "::endgroup::"

echo "::group::Bundle libraries"
# Wine dlopen()s these by file name, which macOS also looks up in the
# calling module's run paths; every unix module gets @loader_path/../../
# (that is, $prefix/lib) below. The Kerberos libraries are the ones kinit
# links, which include the two Wine loads (libkrb5, libgssapi_krb5).
lib="$prefix/lib"
krb5_libs=$(otool -L "$krb5/bin/kinit" | awk '/\/krb5\/.*\.dylib/ {print $1}')
for dylib in \
  "$(brew --prefix freetype)/lib/libfreetype.6.dylib" \
  "$(brew --prefix gnutls)/lib/libgnutls.30.dylib" \
  "$(brew --prefix molten-vk)/lib/libMoltenVK.dylib" \
  "$(brew --prefix sdl2)/lib/libSDL2-2.0.0.dylib" \
  $krb5_libs; do
  cp "$dylib" "$lib/"
  chmod u+w "$lib/$(basename "$dylib")"
done
for dylib in "$lib"/*.dylib; do
  install_name_tool -id "@rpath/$(basename "$dylib")" "$dylib"
  dylibbundler --overwrite-files --bundle-deps --no-codesign \
    --fix-file "$dylib" --dest-dir "$lib" --install-path @loader_path/ >/dev/null
done
# kinit gets a domain account's Kerberos ticket before programs start.
cp "$krb5/bin/kinit" "$prefix/bin/kinit"
chmod u+w "$prefix/bin/kinit"
for dep in $krb5_libs; do
  install_name_tool -change "$dep" "@rpath/$(basename "$dep")" "$prefix/bin/kinit"
done
install_name_tool -add_rpath @executable_path/../lib "$prefix/bin/kinit"
for so in "$prefix"/lib/wine/x86_64-unix/*.so; do
  otool -l "$so" | grep -q "@loader_path/../../" || install_name_tool -add_rpath @loader_path/../../ "$so"
done
# install_name_tool and dylibbundler invalidate signatures; re-sign ad hoc.
find "$prefix" -type f \( -name "*.dylib" -o -name "*.so" -o -perm -u+x \) -print0 |
  xargs -0 -n 50 codesign --force --sign - 2>/dev/null || true
echo "::endgroup::"

echo "::group::Wine Mono and Gecko"
# Bundled like the official macOS builds, so Wine never asks to download them.
addons="$src/dlls/appwiz.cpl/addons.c"
mono=$(sed -n 's/^#define MONO_VERSION "\(.*\)"/\1/p' "$addons")
gecko=$(sed -n 's/^#define GECKO_VERSION "\(.*\)"/\1/p' "$addons")
mkdir -p "$prefix/share/wine/mono" "$prefix/share/wine/gecko"
curl -fsSL "https://dl.winehq.org/wine/wine-mono/$mono/wine-mono-$mono-x86.tar.xz" |
  tar -xJ -C "$prefix/share/wine/mono"
for arch in x86 x86_64; do
  curl -fsSL "https://dl.winehq.org/wine/wine-gecko/$gecko/wine-gecko-$gecko-$arch.tar.xz" |
    tar -xJ -C "$prefix/share/wine/gecko"
done
echo "Wine Mono $mono, Wine Gecko $gecko"
echo "::endgroup::"

echo "::group::Package"
mkdir -p "$prefix/share/vinter"
cp "$src/COPYING.LIB" "$prefix/LICENSE-wine.txt"
cp -R "$here/patches/$patches" "$prefix/share/vinter/patches" 2>/dev/null || true
cat > "$prefix/share/vinter/SOURCE.txt" <<SRC
Vinter Wine $version-$build
Wine source: $source_desc
Changes: the patches in share/vinter/patches, if any (also at
https://github.com/praevit/vinter/tree/main/wine/patches/$patches).
Build script: https://github.com/praevit/vinter/blob/main/wine/build.sh
Wine is licensed under the GNU LGPL, version 2.1 or later (LICENSE-wine.txt).
Includes Wine Mono and Wine Gecko from https://dl.winehq.org/wine/ (their
licenses are inside share/wine/mono and share/wine/gecko).
SRC
"$prefix/bin/wine" --version
archive="$out/$name-macos-x86_64.tar.xz"
tar -C "$work" -cJf "$archive" "$name"
shasum -a 256 "$archive" | tee "$archive.sha256"
echo "::endgroup::"
