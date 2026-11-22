# KiPanel

A PCB panelizer plugin for **KiCad 9+** with a live preview. Open your board, click the KiPanel button, set rows / columns / rails / separation, and export a ready-to-order panel as a new `.kicad_pcb`.

Written in Rust (egui), talks to KiCad through the IPC API. Runs on Windows, Linux and macOS.

![KiPanel window](docs/screenshot.png)

## Features

- **Grid layout** – rows, columns, spacing, board rotation 0 / 90 / 180 / 270°
- **Frame** – none, rails top/bottom, rails left/right, full frame, minimum panel size
- **Separation** – V-cut, tabs + mousebites, or tabs only (tabs are placed only on straight edges)
- **Assembly aids** – tooling holes (3 or 4, NPTH/PTH), fiducials (3 or 4, top/bottom/both), silkscreen panel text
- **Live checks** – V-cut crossing a board, gaps narrower than the router bit, no room for a tab, holes/fiducials off the rail or overlapping, text that does not fit
- **Presets** – proven starting points (mousebites + rails, V-cut + rails, bare boards)
- **Export** – a new board file, your original board is never modified
  - every copy gets fresh UUIDs and its own nets (`Board_N-<net>`)
  - Edge.Cuts is replaced by the panel outline (union of boards, rails and tabs)
  - V-cut lines on a selectable layer, mousebites/tooling holes as NPTH footprints, fiducials as SMD pads
- **Settings per board** – saved as `<project>/<board>.kipanel.json` and loaded again automatically

## Requirements

- KiCad **9.0 or newer**
- The KiCad API server must be on: **Preferences → Plugins → Enable KiCad API** (then restart KiCad)

## Install

### Option A – prebuilt package

If the [Releases](../../releases) page has a zip for your system:

1. Download `kipanel-pcm-windows.zip`, `kipanel-pcm-linux.zip` or `kipanel-pcm-macos.zip`.
2. In KiCad open **Plugin and Content Manager → Install from File…** and pick the zip.
3. Restart KiCad. A **KiPanel** button appears in the PCB editor toolbar.

On Linux / macOS, if the button does nothing after this, the program may have lost its executable flag while unpacking. Fix it with `chmod +x <plugins folder>/com.mubin.kipanel/kipanel` (folders are listed below), or use option B.

### Option B – build from source

You need [Rust](https://rustup.rs), **CMake** and a C compiler (one dependency, the `nng` messaging library, is built from C source).

| System | Prerequisites |
|---|---|
| Windows | Rust, Visual Studio Build Tools ("Desktop development with C++", includes CMake) |
| Debian / Ubuntu | `sudo apt install build-essential cmake zip` + Rust |
| Fedora | `sudo dnf install gcc gcc-c++ make cmake zip` + Rust |
| macOS | `xcode-select --install`, `brew install cmake` + Rust |

Then clone this repository and run the install script. It builds a release binary and copies it into KiCad's plugin folder:

```bash
# Linux / macOS
./install-dev.sh
```

```powershell
# Windows (PowerShell)
.\install-dev.ps1
```

Finally, in the KiCad PCB editor choose **Tools → External Plugins → Refresh Plugins** (or restart KiCad).

Where the plugin is installed:

| System | Folder |
|---|---|
| Windows | `Documents\KiCad\9.0\plugins\com.mubin.kipanel` |
| macOS | `~/Documents/KiCad/9.0/plugins/com.mubin.kipanel` |
| Linux | `~/.local/share/kicad/9.0/plugins/com.mubin.kipanel` |

Other setups:

```bash
KICAD_VERSION=10.0 ./install-dev.sh          # a different KiCad version
.\install-dev.ps1 -KiCadVersion 10.0         # same on Windows
# Flatpak KiCad keeps its data elsewhere:
KICAD_PLUGIN_DIR=~/.var/app/org.kicad.KiCad/data/kicad/9.0/plugins ./install-dev.sh
```

To uninstall, delete the `com.mubin.kipanel` folder.

## Usage

1. Open your board in the KiCad **PCB editor** and click the **KiPanel** toolbar button. KiPanel reads the board as it is right now, including unsaved changes.
2. Optionally pick **Start from preset…** in the toolbar to fill in a sensible combination.
3. Adjust the settings on the right. The preview updates immediately, and hovering a setting highlights what it affects.
   - **Layout** – number of boards and the gaps between them
   - **Frame / rails** – extra material for the assembly line
   - **Separation** – how boards are broken out (V-cut needs 0 mm spacing; mousebites typically 2–3 mm)
   - **Tooling holes & fiducials**, **Panel text**
   - **Output file** – folder, file name postfix, V-cut layer, unique references
4. Watch **Checks** at the bottom right. Red entries are real problems (for example a V-cut running through a board), yellow ones are notes.
5. Click **Export panel**. The result is written to `<project>/panel/<board>_panel.kicad_pcb` by default, and can open in KiCad right away.
6. **Save settings** stores the configuration next to your board, so the next session starts where you left off. The button turns amber with a `*` while there are unsaved changes.

File paths are shown above the Export button and in the status bar after saving or exporting. Click a path to open its folder.

### Preview and shortcuts

| Action | How |
|---|---|
| Pan | drag |
| Zoom | mouse wheel |
| Fit to window | double-click, or **Fit view** |
| Save settings | Ctrl+S (Cmd+S on macOS) |
| Export panel | Ctrl+E (Cmd+E on macOS) |
| Re-read the board from KiCad | **Reload board** |

### Without KiCad (standalone mode)

Pass a board file to try KiPanel without a running KiCad:

```bash
cargo run -- examples/demo.kicad_pcb
```

Everything works except starting from the toolbar; *Open in KiCad* needs `pcbnew` to be installed.

## Troubleshooting

- **"Connect failed"** – the API server is off. Enable it under Preferences → Plugins, restart KiCad, and keep the PCB editor open.
- **"no board open in PCB editor"** – open the board in the PCB editor (not only the project manager), then press **Reload board**.
- **No KiPanel button** – run Tools → External Plugins → Refresh Plugins, and check that the plugin folder above matches your KiCad version.
- **Linux: Browse (…) buttons do nothing** – the file dialogs use the XDG desktop portal; install `xdg-desktop-portal` plus the backend for your desktop (`-gtk`, `-kde`, …). You can always type paths by hand.

## Development

```bash
cargo run                        # needs KiCad with a board open and the API enabled
cargo run -- examples/demo.kicad_pcb   # standalone
cargo test
./package.sh                     # or .\package.ps1 on Windows
```

The package scripts create `dist/com.mubin.kipanel/` (copy into the plugins folder by hand) and `dist/kipanel-pcm-<platform>.zip` (for *Install from File…*). The binary is platform specific, so each system gets its own zip.

GitHub Actions (`.github/workflows/build.yml`) builds and tests on Windows, Linux and macOS for every push and attaches the three zips as artifacts. Pushing a tag such as `v0.1.0` publishes them as a release.

Source layout: `src/board.rs` (outline extraction), `src/panel.rs` (panel geometry and checks), `src/export.rs` (writing the panel board), `src/sexpr.rs` (KiCad file parser), `src/main.rs` (GUI).

## Known limits

- Arcs in the outline are exported as polylines (2° segments)
- Groups and generated items (tuning patterns) are not copied
- Only the largest outline (plus its cutouts) is used; separate outlines are ignored

## License

[MIT](LICENSE)
