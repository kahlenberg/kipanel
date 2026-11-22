#!/usr/bin/env bash
# Builds kipanel and creates two outputs in ./dist (Linux / macOS):
#   1) dist/com.mubin.kipanel/         -> copy into the KiCad plugins folder (manual install)
#   2) dist/kipanel-pcm-<platform>.zip -> KiCad Plugin and Content Manager: "Install from File..."
set -euo pipefail
cd "$(dirname "$0")"

case "$(uname -s)" in
    Darwin) platform="macos" ;;
    Linux)  platform="linux" ;;
    *) echo "Unsupported system '$(uname -s)' - on Windows use package.ps1" >&2; exit 1 ;;
esac
command -v zip >/dev/null || { echo "'zip' is required (e.g. sudo apt install zip)" >&2; exit 1; }

cargo build --release

dist="dist"
plug="$dist/com.mubin.kipanel"
pcm="$dist/pcm"
rm -rf "$dist"
mkdir -p "$plug" "$pcm/plugins" "$pcm/resources"
cp target/release/kipanel pcm/plugin.json pcm/icon_*_24.png pcm/icon_*_48.png "$plug/"
chmod +x "$plug/kipanel"

# PCM layout: metadata.json, plugins/..., resources/icon.png
cp -p "$plug"/* "$pcm/plugins/"
sed "s/@PLATFORM@/$platform/" pcm/metadata.json > "$pcm/metadata.json"
cp pcm/icon_64.png "$pcm/resources/icon.png"
zipfile="$PWD/$dist/kipanel-pcm-$platform.zip"
(cd "$pcm" && zip -qr "$zipfile" .)
rm -rf "$pcm"

echo "Done:"
echo "  manual : $plug"
echo "  PCM zip: $dist/kipanel-pcm-$platform.zip"
