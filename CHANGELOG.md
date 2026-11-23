# Changelog

## 1.0.0

First stable release.

### Panel

- Grid layout with rows, columns, spacing and board rotation in 90° steps; optional 180° flip of every second column or row, alternating spacing, backbones
- Rails top/bottom or left/right, or a full frame; minimum and maximum panel size; chamfered or rounded panel corners; copper fill on the rails
- Separation by V-cut, tabs with mousebites, or tabs only; tabs by count or by distance, rounded inner corners for the router
- Tooling holes, fiducials and up to four panel texts with placeholders
- Presets for common combinations

### Preview

- Live 2D preview; hovering a setting highlights what it affects
- 3D preview of the panel with the components, copper and silkscreen of the board, in the colours of KiCad's 3D viewer
- Live checks with a **Fix automatically** button that lists its changes and can be undone

### Export

- Writes a new `.kicad_pcb`; the original board is never modified
- Fresh UUIDs and separate nets per board copy, optional unique references, V-cut lines on a selectable layer
- After export: design rule check with the findings marked in the preview, 3D picture, 3D model (STEP, GLB, STL, PLY, VRML, BREP, XAO)
- Settings are saved per board as `<board>.kipanel.json`

### Platforms

- Windows, Linux and macOS (Apple Silicon) packages for KiCad's Plugin and Content Manager, KiCad 9.0 or newer
