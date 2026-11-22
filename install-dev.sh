#!/usr/bin/env bash
# Build and copy straight into the KiCad plugin folder (Linux / macOS).
#   ./install-dev.sh                      # KiCad 9.0
#   KICAD_VERSION=10.0 ./install-dev.sh   # other KiCad version
#   KICAD_PLUGIN_DIR=/path ./install-dev.sh   # custom plugin folder (e.g. Flatpak, see README)
set -euo pipefail
cd "$(dirname "$0")"

ver="${KICAD_VERSION:-9.0}"
case "$(uname -s)" in
    Darwin) base="$HOME/Documents/KiCad/$ver/plugins" ;;
    Linux)  base="${XDG_DATA_HOME:-$HOME/.local/share}/kicad/$ver/plugins" ;;
    *) echo "Unsupported system '$(uname -s)' - on Windows use install-dev.ps1" >&2; exit 1 ;;
esac
dst="${KICAD_PLUGIN_DIR:-$base}/com.mubin.kipanel"

cargo build --release

mkdir -p "$dst"
cp target/release/kipanel pcm/plugin.json pcm/icon_*_24.png pcm/icon_*_48.png "$dst/"
chmod +x "$dst/kipanel"
echo "Installed to $dst"
echo "  -> in KiCad (PCB editor): Tools > External Plugins > Refresh Plugins"
