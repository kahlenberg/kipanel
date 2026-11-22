//! Panel layout: grid placement of board copies + rails / frame.

use crate::board::{Outline, Pt};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Framing {
    None,
    RailsTB,
    RailsLR,
    Frame,
}

impl Framing {
    pub const ALL: [Framing; 4] = [Framing::None, Framing::RailsTB, Framing::RailsLR, Framing::Frame];
    pub fn label(self) -> &'static str {
        match self {
            Framing::None => "None",
            Framing::RailsTB => "Rails top / bottom",
            Framing::RailsLR => "Rails left / right",
            Framing::Frame => "Full frame",
        }
    }
    pub fn has_tb(self) -> bool {
        matches!(self, Framing::RailsTB | Framing::Frame)
    }
    pub fn has_lr(self) -> bool {
        matches!(self, Framing::RailsLR | Framing::Frame)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelConfig {
    pub rows: u32,
    pub cols: u32,
    /// gap between columns (mm)
    pub hspace: f64,
    /// gap between rows (mm)
    pub vspace: f64,
    /// board rotation in degrees (0/90/180/270)
    pub rotation: u32,
    pub framing: Framing,
    /// rail / frame width (mm)
    pub rail_width: f64,
    /// gap between rails and boards (mm)
    pub rail_space: f64,
    /// minimum total panel size (mm), 0 = off
    pub min_w: f64,
    pub min_h: f64,
    // --- separation ---
    pub sep: Separation,
    /// tab width along the board edge (mm)
    pub tab_width: f64,
    /// tabs per top / bottom edge
    pub tabs_h: u32,
    /// tabs per left / right edge
    pub tabs_v: u32,
    /// mousebite drill diameter (mm)
    pub mb_drill: f64,
    /// mousebite hole pitch, center to center (mm)
    pub mb_spacing: f64,
    /// mousebite hole center offset into the board (mm)
    pub mb_offset: f64,
    /// fab router (milling bit) diameter (mm)
    pub mill_d: f64,
    // --- tooling holes / fiducials / text ---
    pub tooling: Corners,
    pub tool_d: f64,
    /// distance of feature center from the panel corner, along the rail (mm)
    pub tool_along: f64,
    pub tool_pth: bool,
    pub fiducials: Corners,
    pub fid_copper: f64,
    pub fid_opening: f64,
    pub fid_along: f64,
    pub fid_side: FidSide,
    pub text: String,
    pub text_size: f64,
    pub text_pos: TextPos,
    // --- export ---
    pub vcut_layer: String,
    pub rename_refs: bool,
    /// position of the panel's top-left corner in the output file (mm)
    pub origin: (f64, f64),
    // --- files ---
    /// output folder; empty = <project>/panel, relative = relative to project folder
    pub out_dir: String,
    /// appended to the board name: <board><postfix>.kicad_pcb
    pub out_postfix: String,
    /// copy the .kicad_pro next to the panel (design rules, net classes)
    pub copy_pro: bool,
    /// open the panel in KiCad after export
    pub open_after: bool,
    /// settings file; empty = <project>/<board>.kipanel.json
    pub settings_file: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    MousebitesRails,
    VCutRails,
    VCutBare,
    MousebitesBare,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::MousebitesRails, Preset::VCutRails, Preset::VCutBare, Preset::MousebitesBare];
    pub fn label(self) -> &'static str {
        match self {
            Preset::MousebitesRails => "Mousebites + rails (for assembly)",
            Preset::VCutRails => "V-cut + rails (for assembly)",
            Preset::VCutBare => "V-cut, no rails (bare boards)",
            Preset::MousebitesBare => "Mousebites, no rails (bare boards)",
        }
    }
    /// apply to a config; keeps grid size, rotation and file settings
    pub fn apply(self, c: &mut PanelConfig) {
        let d = PanelConfig::default();
        let keep = (c.rows, c.cols, c.rotation, c.out_dir.clone(), c.out_postfix.clone(), c.copy_pro, c.open_after, c.settings_file.clone(), c.text.clone());
        *c = d;
        (c.rows, c.cols, c.rotation, c.out_dir, c.out_postfix, c.copy_pro, c.open_after, c.settings_file, c.text) = keep;
        match self {
            Preset::MousebitesRails => {}
            Preset::VCutRails => {
                c.sep = Separation::VCut;
                c.hspace = 0.0;
                c.vspace = 0.0;
                c.rail_space = 0.0;
            }
            Preset::VCutBare => {
                c.sep = Separation::VCut;
                c.hspace = 0.0;
                c.vspace = 0.0;
                c.rail_space = 0.0;
                c.framing = Framing::None;
                c.tooling = Corners::None;
                c.fiducials = Corners::None;
            }
            Preset::MousebitesBare => {
                c.framing = Framing::None;
                c.tooling = Corners::None;
                c.fiducials = Corners::None;
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Corners {
    None,
    Three,
    Four,
}

impl Corners {
    pub const ALL: [Corners; 3] = [Corners::None, Corners::Three, Corners::Four];
    pub fn label(self) -> &'static str {
        match self {
            Corners::None => "None",
            Corners::Three => "3 (asymmetric)",
            Corners::Four => "4 (all corners)",
        }
    }
    fn count(self) -> usize {
        match self {
            Corners::None => 0,
            Corners::Three => 3,
            Corners::Four => 4,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FidSide {
    Top,
    Bottom,
    Both,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TextPos {
    TopRail,
    BottomRail,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Separation {
    VCut,
    Mousebites,
    Tabs,
}

impl Separation {
    pub const ALL: [Separation; 3] = [Separation::VCut, Separation::Mousebites, Separation::Tabs];
    pub fn label(self) -> &'static str {
        match self {
            Separation::VCut => "V-cut (scored lines)",
            Separation::Mousebites => "Tabs + mousebites",
            Separation::Tabs => "Tabs only (break / cut)",
        }
    }
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            rows: 1,
            cols: 2,
            hspace: 2.0,
            vspace: 2.0,
            rotation: 0,
            framing: Framing::RailsTB,
            rail_width: 5.0,
            rail_space: 2.0,
            min_w: 0.0,
            min_h: 0.0,
            sep: Separation::Mousebites,
            tab_width: 5.0,
            tabs_h: 1,
            tabs_v: 2,
            mb_drill: 0.5,
            mb_spacing: 0.8,
            mb_offset: 0.25,
            mill_d: 2.0,
            tooling: Corners::Three,
            tool_d: 1.152,
            tool_along: 5.0,
            tool_pth: false,
            fiducials: Corners::Three,
            fid_copper: 1.0,
            fid_opening: 2.0,
            fid_along: 10.0,
            fid_side: FidSide::Top,
            text: String::new(),
            text_size: 1.0,
            text_pos: TextPos::BottomRail,
            vcut_layer: "Cmts.User".into(),
            rename_refs: false,
            origin: (20.0, 20.0),
            out_dir: String::new(),
            out_postfix: "_panel".into(),
            copy_pro: true,
            open_after: false,
            settings_file: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RectMm {
    pub min: Pt,
    pub max: Pt,
}

#[allow(dead_code)]
impl RectMm {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { min: (x0.min(x1), y0.min(y1)), max: (x0.max(x1), y0.max(y1)) }
    }
    pub fn w(&self) -> f64 {
        self.max.0 - self.min.0
    }
    pub fn h(&self) -> f64 {
        self.max.1 - self.min.1
    }
}

/// One placed board copy: transformed loops (panel coordinates, mm, origin = panel top-left).
pub struct Placed {
    pub loops: Vec<Vec<Pt>>,
    pub bbox: RectMm,
    pub row: u32,
    pub col: u32,
    /// original board coords -> panel coords: rotate(p, rot_center, rotation) + shift
    pub shift: Pt,
}

/// Side index: 0 = top, 1 = right, 2 = bottom, 3 = left
pub const TOP: usize = 0;
pub const RIGHT: usize = 1;
pub const BOTTOM: usize = 2;
pub const LEFT: usize = 3;

pub struct Panel {
    pub size: (f64, f64),
    pub boards: Vec<Placed>,
    /// rail / frame pieces
    pub rails: Vec<RectMm>,
    /// gaps between columns / rows (for highlighting)
    pub hgaps: Vec<RectMm>,
    pub vgaps: Vec<RectMm>,
    /// gap between rails and board area
    pub rail_gaps: Vec<RectMm>,
    /// board area (all copies incl. gaps)
    pub area: RectMm,
    pub rows: u32,
    pub cols: u32,
    /// size of one (rotated) board
    pub board_size: (f64, f64),
    /// per side: intervals (along the side, relative to board bbox min) where the
    /// outline lies straight on the bbox line -> usable for tabs
    pub sides: [Vec<(f64, f64)>; 4],
    /// outer outline is (nearly) a rectangle
    pub rectangular: bool,
    pub rot_center: Pt,
    pub rotation: u32,
    /// actual rail widths (after min-size growth), 0 if absent
    pub rail_tb_w: f64,
    pub rail_lr_w: f64,
}

impl Panel {
    /// original board coordinate -> panel coordinate for board copy `i`
    pub fn to_panel(&self, i: usize, p: Pt) -> Pt {
        let r = rotate(p, self.rot_center, self.rotation);
        let s = self.boards[i].shift;
        (r.0 + s.0, r.1 + s.1)
    }
}

pub fn rotate(p: Pt, c: Pt, deg: u32) -> Pt {
    let (dx, dy) = (p.0 - c.0, p.1 - c.1);
    // KiCad: y down, positive rotation = counter-clockwise on screen
    match deg % 360 {
        90 => (c.0 + dy, c.1 - dx),
        180 => (c.0 - dx, c.1 - dy),
        270 => (c.0 - dy, c.1 + dx),
        _ => p,
    }
}

pub fn build(o: &Outline, cfg: &PanelConfig) -> Panel {
    let rows = cfg.rows.max(1);
    let cols = cfg.cols.max(1);
    let c = ((o.min.0 + o.max.0) / 2.0, (o.min.1 + o.max.1) / 2.0);

    // rotated board, normalized so its bbox starts at (0,0)
    let rot_loops: Vec<Vec<Pt>> = o
        .loops
        .iter()
        .map(|l| l.iter().map(|&p| rotate(p, c, cfg.rotation)).collect())
        .collect();
    let (mut mn, mut mx) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for &(x, y) in rot_loops.iter().flatten() {
        mn = (mn.0.min(x), mn.1.min(y));
        mx = (mx.0.max(x), mx.1.max(y));
    }
    let (bw, bh) = (mx.0 - mn.0, mx.1 - mn.1);

    let area_w = cols as f64 * bw + (cols - 1) as f64 * cfg.hspace;
    let area_h = rows as f64 * bh + (rows - 1) as f64 * cfg.vspace;

    let (rw, rs) = (cfg.rail_width, cfg.rail_space);
    let tb = cfg.framing.has_tb();
    let lr = cfg.framing.has_lr();

    // rail widths, possibly enlarged to meet minimum panel size
    let mut rail_tb = if tb { rw } else { 0.0 };
    let mut rail_lr = if lr { rw } else { 0.0 };
    let base_h = area_h + if tb { 2.0 * (rs + rail_tb) } else { 0.0 };
    let base_w = area_w + if lr { 2.0 * (rs + rail_lr) } else { 0.0 };
    if tb && cfg.min_h > base_h {
        rail_tb += (cfg.min_h - base_h) / 2.0;
    }
    if lr && cfg.min_w > base_w {
        rail_lr += (cfg.min_w - base_w) / 2.0;
    }
    // rails TB only: panel width can still grow (rails wider than boards)
    let mut panel_w = area_w + if lr { 2.0 * (rs + rail_lr) } else { 0.0 };
    let mut panel_h = area_h + if tb { 2.0 * (rs + rail_tb) } else { 0.0 };
    if tb && !lr && cfg.min_w > panel_w {
        panel_w = cfg.min_w;
    }
    if lr && !tb && cfg.min_h > panel_h {
        panel_h = cfg.min_h;
    }

    // board area origin (centered)
    let ox = (panel_w - area_w) / 2.0;
    let oy = (panel_h - area_h) / 2.0;
    let area = RectMm::new(ox, oy, ox + area_w, oy + area_h);

    let mut boards = Vec::new();
    for r in 0..rows {
        for col in 0..cols {
            let x0 = ox + col as f64 * (bw + cfg.hspace);
            let y0 = oy + r as f64 * (bh + cfg.vspace);
            let loops = rot_loops
                .iter()
                .map(|l| l.iter().map(|&(x, y)| (x - mn.0 + x0, y - mn.1 + y0)).collect())
                .collect();
            boards.push(Placed {
                loops,
                bbox: RectMm::new(x0, y0, x0 + bw, y0 + bh),
                row: r,
                col,
                shift: (x0 - mn.0, y0 - mn.1),
            });
        }
    }

    let mut hgaps = Vec::new();
    for col in 1..cols {
        let x = ox + col as f64 * bw + (col - 1) as f64 * cfg.hspace;
        hgaps.push(RectMm::new(x, oy, x + cfg.hspace, oy + area_h));
    }
    let mut vgaps = Vec::new();
    for r in 1..rows {
        let y = oy + r as f64 * bh + (r - 1) as f64 * cfg.vspace;
        vgaps.push(RectMm::new(ox, y, ox + area_w, y + cfg.vspace));
    }

    let mut rails = Vec::new();
    let mut rail_gaps = Vec::new();
    if tb {
        rails.push(RectMm::new(0.0, 0.0, panel_w, rail_tb));
        rails.push(RectMm::new(0.0, panel_h - rail_tb, panel_w, panel_h));
        rail_gaps.push(RectMm::new(ox, oy - rs, ox + area_w, oy));
        rail_gaps.push(RectMm::new(ox, oy + area_h, ox + area_w, oy + area_h + rs));
    }
    if lr {
        let (y0, y1) = if tb { (rail_tb, panel_h - rail_tb) } else { (0.0, panel_h) };
        rails.push(RectMm::new(0.0, y0, rail_lr, y1));
        rails.push(RectMm::new(panel_w - rail_lr, y0, panel_w, y1));
        rail_gaps.push(RectMm::new(ox - rs, oy, ox, oy + area_h));
        rail_gaps.push(RectMm::new(ox + area_w, oy, ox + area_w + rs, oy + area_h));
    }

    // straight-edge intervals per side (board-relative)
    let norm: Vec<Pt> = rot_loops
        .first()
        .map(|l| l.iter().map(|&(x, y)| (x - mn.0, y - mn.1)).collect())
        .unwrap_or_default();
    let sides = side_intervals(&norm, bw, bh);
    let rectangular = polygon_area(&norm) > bw * bh * 0.999;

    Panel {
        size: (panel_w, panel_h),
        boards,
        rails,
        hgaps,
        vgaps,
        rail_gaps,
        area,
        rows,
        cols,
        board_size: (bw, bh),
        sides,
        rectangular,
        rot_center: c,
        rotation: cfg.rotation % 360,
        rail_tb_w: if tb { rail_tb } else { 0.0 },
        rail_lr_w: if lr { rail_lr } else { 0.0 },
    }
}

// ---------------------------------------------------------------- features

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeatureKind {
    Tooling,
    FiducialTop,
    FiducialBottom,
}

#[derive(Clone, Debug)]
pub struct Feature {
    pub kind: FeatureKind,
    pub pos: Pt,
    /// keep-out radius (hole radius / mask opening radius)
    pub r: f64,
    pub corner: &'static str,
}

#[derive(Clone, Debug)]
pub struct PanelText {
    pub pos: Pt,
    pub text: String,
    pub size: f64,
    /// estimated extents for checks / preview
    pub rect: RectMm,
}

pub struct Features {
    pub items: Vec<Feature>,
    pub text: Option<PanelText>,
    pub warnings: Vec<String>,
}

fn corner_pos(p: &Panel, cfg: &PanelConfig, corner: usize, along: f64) -> Pt {
    let (pw, ph) = p.size;
    let right = corner == 1 || corner == 3;
    let bottom = corner == 2 || corner == 3;
    if cfg.framing.has_tb() {
        let x = if right { pw - along } else { along };
        let y = if bottom { ph - p.rail_tb_w / 2.0 } else { p.rail_tb_w / 2.0 };
        (x, y)
    } else if cfg.framing.has_lr() {
        let x = if right { pw - p.rail_lr_w / 2.0 } else { p.rail_lr_w / 2.0 };
        let y = if bottom { ph - along } else { along };
        (x, y)
    } else {
        (if right { pw - along } else { along }, if bottom { ph - along } else { along })
    }
}

const CORNER_NAMES: [&str; 4] = ["top-left", "top-right", "bottom-left", "bottom-right"];

pub fn features(p: &Panel, cfg: &PanelConfig, vcuts: &[VCutLine]) -> Features {
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    for corner in 0..cfg.tooling.count() {
        items.push(Feature {
            kind: FeatureKind::Tooling,
            pos: corner_pos(p, cfg, corner, cfg.tool_along),
            r: cfg.tool_d / 2.0,
            corner: CORNER_NAMES[corner],
        });
    }
    for corner in 0..cfg.fiducials.count() {
        let pos = corner_pos(p, cfg, corner, cfg.fid_along);
        let kinds: &[FeatureKind] = match cfg.fid_side {
            FidSide::Top => &[FeatureKind::FiducialTop],
            FidSide::Bottom => &[FeatureKind::FiducialBottom],
            FidSide::Both => &[FeatureKind::FiducialTop, FeatureKind::FiducialBottom],
        };
        for &kind in kinds {
            items.push(Feature { kind, pos, r: cfg.fid_opening.max(cfg.fid_copper) / 2.0, corner: CORNER_NAMES[corner] });
        }
    }

    // must sit fully on a rail
    let inside_rail = |c: Pt, r: f64| {
        p.rails.iter().any(|rr| {
            c.0 - r >= rr.min.0 - ETOL && c.0 + r <= rr.max.0 + ETOL && c.1 - r >= rr.min.1 - ETOL && c.1 + r <= rr.max.1 + ETOL
        })
    };
    let kind_name = |k: FeatureKind| match k {
        FeatureKind::Tooling => "Tooling hole",
        _ => "Fiducial",
    };
    for f in &items {
        if !inside_rail(f.pos, f.r) {
            warnings.push(format!(
                "⚠ {} {} does not fit on a rail (Ø {:.2} mm). Use rails, make them wider, or change the corner distance.",
                kind_name(f.kind), f.corner, 2.0 * f.r
            ));
        }
        for v in vcuts {
            let d = if v.vertical { (f.pos.0 - v.pos).abs() } else { (f.pos.1 - v.pos).abs() };
            if d < f.r + 0.3 {
                warnings.push(format!("⚠ V-cut line runs through {} {}.", kind_name(f.kind).to_lowercase(), f.corner));
            }
        }
    }
    for i in 0..items.len() {
        for j in i + 1..items.len() {
            let (a, b) = (&items[i], &items[j]);
            if a.pos == b.pos && a.kind != FeatureKind::Tooling && b.kind != FeatureKind::Tooling {
                continue; // top + bottom fiducial at same spot
            }
            let d = ((a.pos.0 - b.pos.0).powi(2) + (a.pos.1 - b.pos.1).powi(2)).sqrt();
            if d < a.r + b.r + 0.5 {
                warnings.push(format!(
                    "⚠ {} {} overlaps {} {} — increase the corner distance of one of them.",
                    kind_name(a.kind), a.corner, kind_name(b.kind).to_lowercase(), b.corner
                ));
            }
        }
    }

    // text on rail center
    let mut text = None;
    let t = cfg.text.trim();
    if !t.is_empty() {
        let (pw, ph) = p.size;
        let on_tb = cfg.framing.has_tb();
        let pos = if on_tb {
            let y = match cfg.text_pos {
                TextPos::TopRail => p.rail_tb_w / 2.0,
                TextPos::BottomRail => ph - p.rail_tb_w / 2.0,
            };
            (pw / 2.0, y)
        } else if cfg.framing.has_lr() {
            let x = match cfg.text_pos {
                TextPos::TopRail => p.rail_lr_w / 2.0,
                TextPos::BottomRail => pw - p.rail_lr_w / 2.0,
            };
            (x, ph / 2.0)
        } else {
            (pw / 2.0, ph + cfg.text_size)
        };
        // rough KiCad stroke font width: ~0.9 × size per char
        let w = t.chars().count() as f64 * cfg.text_size * 0.9;
        let h = cfg.text_size;
        let rect = RectMm::new(pos.0 - w / 2.0, pos.1 - h / 2.0, pos.0 + w / 2.0, pos.1 + h / 2.0);
        if !inside_rail(pos, 0.0)
            || !p.rails.iter().any(|r| rect.min.0 >= r.min.0 - ETOL && rect.max.0 <= r.max.0 + ETOL && rect.min.1 >= r.min.1 - ETOL && rect.max.1 <= r.max.1 + ETOL)
        {
            warnings.push("⚠ Panel text does not fit on the rail (too long / too big, or no rail).".into());
        }
        if items.iter().any(|f| {
            f.pos.0 + f.r > rect.min.0 && f.pos.0 - f.r < rect.max.0 && f.pos.1 + f.r > rect.min.1 && f.pos.1 - f.r < rect.max.1
        }) {
            warnings.push("⚠ Panel text overlaps a tooling hole or fiducial.".into());
        }
        if !cfg.framing.has_tb() && cfg.framing.has_lr() {
            warnings.push("Text on left/right rails is placed horizontally — may not fit narrow rails.".into());
        }
        text = Some(PanelText { pos, text: t.to_string(), size: cfg.text_size, rect });
    }
    if cfg.tooling != Corners::None && cfg.framing == Framing::None {
        warnings.push("Tooling holes / fiducials need rails or a frame.".into());
    }
    Features { items, text, warnings }
}

const ETOL: f64 = 1e-3;

fn polygon_area(p: &[Pt]) -> f64 {
    let n = p.len();
    if n < 3 {
        return 0.0;
    }
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

fn merge(mut v: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (a, b) in v {
        if let Some(last) = out.last_mut() {
            if a <= last.1 + ETOL {
                last.1 = last.1.max(b);
                continue;
            }
        }
        out.push((a, b));
    }
    out
}

fn side_intervals(p: &[Pt], bw: f64, bh: f64) -> [Vec<(f64, f64)>; 4] {
    let mut s: [Vec<(f64, f64)>; 4] = Default::default();
    let n = p.len();
    for i in 0..n {
        let (a, b) = (p[i], p[(i + 1) % n]);
        let on = |v: f64, t: f64| (v - t).abs() < ETOL;
        if on(a.1, 0.0) && on(b.1, 0.0) {
            s[TOP].push((a.0.min(b.0), a.0.max(b.0)));
        } else if on(a.1, bh) && on(b.1, bh) {
            s[BOTTOM].push((a.0.min(b.0), a.0.max(b.0)));
        } else if on(a.0, 0.0) && on(b.0, 0.0) {
            s[LEFT].push((a.1.min(b.1), a.1.max(b.1)));
        } else if on(a.0, bw) && on(b.0, bw) {
            s[RIGHT].push((a.1.min(b.1), a.1.max(b.1)));
        }
    }
    s.map(merge)
}

fn intersect(a: &[(f64, f64)], b: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for &(a0, a1) in a {
        for &(b0, b1) in b {
            let (lo, hi) = (a0.max(b0), a1.min(b1));
            if hi > lo + ETOL {
                out.push((lo, hi));
            }
        }
    }
    out
}

// ---------------------------------------------------------------- separation

#[derive(Clone, Debug)]
pub struct VCutLine {
    pub vertical: bool,
    /// x for vertical, y for horizontal (panel mm)
    pub pos: f64,
    /// crosses a board -> invalid
    pub conflict: bool,
}

#[derive(Default)]
pub struct Sep {
    pub vcuts: Vec<VCutLine>,
    /// tab bridges (substrate)
    pub tabs: Vec<RectMm>,
    /// extra substrate (gap strips filled for V-cut with spacing > 0)
    pub fills: Vec<RectMm>,
    /// mousebite holes: (center, diameter)
    pub holes: Vec<(Pt, f64)>,
    /// break lines (tabs-only mode)
    pub breaks: Vec<(Pt, Pt)>,
    pub warnings: Vec<String>,
}

/// Choose tab centers along a side.
/// `span` = side extent, `usable` = intervals where a tab may sit, `n` = desired count, `w` = width
fn tab_centers(span: (f64, f64), usable: &[(f64, f64)], n: u32, w: f64) -> Vec<f64> {
    let ranges: Vec<(f64, f64)> = usable
        .iter()
        .filter(|(a, b)| b - a >= w - ETOL)
        .map(|&(a, b)| (a + w / 2.0, b - w / 2.0))
        .collect();
    if ranges.is_empty() || n == 0 {
        return vec![];
    }
    let mut out: Vec<f64> = Vec::new();
    for k in 0..n {
        let want = span.0 + (k as f64 + 0.5) * (span.1 - span.0) / n as f64;
        // snap into nearest feasible range
        let best = ranges
            .iter()
            .map(|&(lo, hi)| want.clamp(lo, hi))
            .min_by(|a, b| (a - want).abs().partial_cmp(&(b - want).abs()).unwrap())
            .unwrap();
        if out.iter().all(|&c| (c - best).abs() >= w) {
            out.push(best);
        }
    }
    out
}

pub fn separation(p: &Panel, cfg: &PanelConfig) -> Sep {
    match cfg.sep {
        Separation::VCut => vcut(p, cfg),
        Separation::Mousebites | Separation::Tabs => tabs(p, cfg),
    }
}

fn vcut(p: &Panel, cfg: &PanelConfig) -> Sep {
    let mut s = Sep::default();
    let (pw, ph) = p.size;
    let mut xs: Vec<f64> = Vec::new();
    let mut ys: Vec<f64> = Vec::new();
    for b in &p.boards {
        xs.extend([b.bbox.min.0, b.bbox.max.0]);
        ys.extend([b.bbox.min.1, b.bbox.max.1]);
    }
    let dedup = |mut v: Vec<f64>, limit: f64| -> Vec<f64> {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v.dedup_by(|a, b| (*a - *b).abs() < ETOL);
        v.into_iter().filter(|&x| x > ETOL && x < limit - ETOL).collect()
    };
    for (vertical, list, limit) in [(true, dedup(xs, pw), pw), (false, dedup(ys, ph), ph)] {
        let _ = limit;
        for pos in list {
            let conflict = p.boards.iter().any(|b| {
                let (lo, hi) = if vertical { (b.bbox.min.0, b.bbox.max.0) } else { (b.bbox.min.1, b.bbox.max.1) };
                pos > lo + ETOL && pos < hi - ETOL
            });
            s.vcuts.push(VCutLine { vertical, pos, conflict });
        }
    }

    // gaps > 0: fill them so everything stays connected; V-cut on both sides of each gap
    let any_gap = (p.cols > 1 && cfg.hspace > ETOL) || (p.rows > 1 && cfg.vspace > ETOL);
    let rail_gap = cfg.framing != Framing::None && cfg.rail_space > ETOL;
    if any_gap || rail_gap {
        s.fills.extend(p.hgaps.iter().copied());
        s.fills.extend(p.vgaps.iter().copied());
        s.fills.extend(p.rail_gaps.iter().copied());
        s.warnings.push("Spacing > 0 with V-cut: each gap gets two V-cuts and the strip between breaks out as waste. Use 0 mm for clean edges.".into());
    }
    if s.vcuts.iter().any(|v| v.conflict) {
        s.warnings.push("⚠ V-cut line crosses a board (red). V-cuts run across the whole panel — change layout or use mousebites.".into());
    }
    if p.size.0 < 70.0 - ETOL || p.size.1 < 70.0 - ETOL {
        s.warnings.push(format!(
            "Panel is {:.1} × {:.1} mm. Many fabs only V-cut panels ≥ 70 × 70 mm — use Min panel width/height if needed.",
            p.size.0, p.size.1
        ));
    }
    if !p.rectangular {
        s.warnings.push("Board is not rectangular: only bounding-box edges are V-cut, all other edges (notches, arcs, shoulders) are milled by the fab.".into());
    }
    s
}

fn tabs(p: &Panel, cfg: &PanelConfig) -> Sep {
    let mut s = Sep::default();
    let w = cfg.tab_width;
    let (bw, bh) = p.board_size;
    let sides = &p.sides;
    let idx = |r: u32, c: u32| (r * p.cols + c) as usize;

    // push a horizontal-gap tab (bridge in X direction) / vertical-gap tab
    // returns nothing; adds tab rect + holes/breaks
    let add_tab = |s: &mut Sep, rect: RectMm, cut_lines: Vec<(Pt, Pt, Pt)>| {
        s.tabs.push(rect);
        for (a, b, into) in cut_lines {
            // a..b = cut along board edge, `into` = unit vector pointing into the board
            match cfg.sep {
                Separation::Mousebites => {
                    let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                    let n = ((len - cfg.mb_drill) / cfg.mb_spacing).floor().max(0.0) as usize + 1;
                    let used = (n - 1) as f64 * cfg.mb_spacing;
                    let start = (len - used) / 2.0;
                    let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
                    for i in 0..n {
                        let t = start + i as f64 * cfg.mb_spacing;
                        let c = (
                            a.0 + ux * t + into.0 * cfg.mb_offset,
                            a.1 + uy * t + into.1 * cfg.mb_offset,
                        );
                        s.holes.push((c, cfg.mb_drill));
                    }
                }
                _ => s.breaks.push((a, b)),
            }
        }
    };

    let mut missing: Vec<String> = Vec::new();
    let longest = |v: &[(f64, f64)]| v.iter().map(|(a, b)| b - a).fold(0.0_f64, f64::max);
    let side_name = ["top", "right", "bottom", "left"];
    for b in &p.boards {
        let (x0, y0) = b.bbox.min;
        let (x1, y1) = b.bbox.max;

        // ---- right side: neighbour board or right rail
        let right_n = if b.col + 1 < p.cols { Some(&p.boards[idx(b.row, b.col + 1)]) } else { None };
        let right_rail = b.col + 1 == p.cols && cfg.framing.has_lr();
        if right_n.is_some() || right_rail {
            let gap = if right_n.is_some() { cfg.hspace } else { cfg.rail_space };
            if gap > ETOL {
                let usable = match right_n {
                    Some(_) => intersect(&sides[RIGHT], &sides[LEFT]),
                    None => sides[RIGHT].clone(),
                };
                let cs = tab_centers((0.0, bh), &usable, cfg.tabs_v, w);
                if cs.is_empty() && cfg.tabs_v > 0 { missing.push(format!("board {} {} to {}: longest straight {:.2} mm", idx(b.row, b.col) + 1, side_name[RIGHT], if right_n.is_some() { "next board" } else { "rail" }, longest(&usable))); }
                for c in cs {
                    let (ya, yb) = (y0 + c - w / 2.0, y0 + c + w / 2.0);
                    let mut cuts = vec![((x1, ya), (x1, yb), (-1.0, 0.0))];
                    if right_n.is_some() {
                        cuts.push(((x1 + gap, ya), (x1 + gap, yb), (1.0, 0.0)));
                    }
                    add_tab(&mut s, RectMm::new(x1, ya, x1 + gap, yb), cuts);
                }
            }
        }
        // ---- left rail
        if b.col == 0 && cfg.framing.has_lr() && cfg.rail_space > ETOL {
            let cs = tab_centers((0.0, bh), &sides[LEFT], cfg.tabs_v, w);
            if cs.is_empty() && cfg.tabs_v > 0 { missing.push(format!("board {} {} to rail: longest straight {:.2} mm", idx(b.row, b.col) + 1, side_name[LEFT], longest(&sides[LEFT]))); }
            for c in cs {
                let (ya, yb) = (y0 + c - w / 2.0, y0 + c + w / 2.0);
                add_tab(&mut s, RectMm::new(x0 - cfg.rail_space, ya, x0, yb), vec![((x0, ya), (x0, yb), (1.0, 0.0))]);
            }
        }
        // ---- bottom side: neighbour board or bottom rail
        let bot_n = if b.row + 1 < p.rows { Some(&p.boards[idx(b.row + 1, b.col)]) } else { None };
        let bot_rail = b.row + 1 == p.rows && cfg.framing.has_tb();
        if bot_n.is_some() || bot_rail {
            let gap = if bot_n.is_some() { cfg.vspace } else { cfg.rail_space };
            if gap > ETOL {
                let usable = match bot_n {
                    Some(_) => intersect(&sides[BOTTOM], &sides[TOP]),
                    None => sides[BOTTOM].clone(),
                };
                let cs = tab_centers((0.0, bw), &usable, cfg.tabs_h, w);
                if cs.is_empty() && cfg.tabs_h > 0 { missing.push(format!("board {} {} to {}: longest straight {:.2} mm", idx(b.row, b.col) + 1, side_name[BOTTOM], if bot_n.is_some() { "next board" } else { "rail" }, longest(&usable))); }
                for c in cs {
                    let (xa, xb) = (x0 + c - w / 2.0, x0 + c + w / 2.0);
                    let mut cuts = vec![((xa, y1), (xb, y1), (0.0, -1.0))];
                    if bot_n.is_some() {
                        cuts.push(((xa, y1 + gap), (xb, y1 + gap), (0.0, 1.0)));
                    }
                    add_tab(&mut s, RectMm::new(xa, y1, xb, y1 + gap), cuts);
                }
            }
        }
        // ---- top rail
        if b.row == 0 && cfg.framing.has_tb() && cfg.rail_space > ETOL {
            let cs = tab_centers((0.0, bw), &sides[TOP], cfg.tabs_h, w);
            if cs.is_empty() && cfg.tabs_h > 0 { missing.push(format!("board {} {} to rail: longest straight {:.2} mm", idx(b.row, b.col) + 1, side_name[TOP], longest(&sides[TOP]))); }
            for c in cs {
                let (xa, xb) = (x0 + c - w / 2.0, x0 + c + w / 2.0);
                add_tab(&mut s, RectMm::new(xa, y0 - cfg.rail_space, xb, y0), vec![((xa, y0), (xb, y0), (0.0, 1.0))]);
            }
        }
    }

    // ---- rules
    let gaps_zero = (p.cols > 1 && cfg.hspace <= ETOL)
        || (p.rows > 1 && cfg.vspace <= ETOL)
        || (cfg.framing != Framing::None && cfg.rail_space <= ETOL);
    if gaps_zero {
        s.warnings.push("⚠ A spacing is 0 mm: no room for tabs there. Use ≥ router Ø, or switch to V-cut.".into());
    }
    let min_gap = [
        if p.cols > 1 { cfg.hspace } else { f64::MAX },
        if p.rows > 1 { cfg.vspace } else { f64::MAX },
        if cfg.framing != Framing::None { cfg.rail_space } else { f64::MAX },
    ]
    .into_iter()
    .filter(|g| *g > ETOL)
    .fold(f64::MAX, f64::min);
    if min_gap < cfg.mill_d - ETOL {
        s.warnings.push(format!("⚠ Gap {min_gap:.2} mm is smaller than the router Ø {:.2} mm — the fab cannot mill between the boards.", cfg.mill_d));
    }
    if !missing.is_empty() {
        s.warnings.push(format!(
            "⚠ No straight section ≥ {w:.2} mm (tab width) on {} edge(s) — no tab placed there:\n  • {}\nReduce tab width below the longest straight length.",
            missing.len(),
            missing.join("\n  • ")
        ));
    }
    if s.tabs.is_empty() && cfg.framing != Framing::None {
        s.warnings.push("⚠ No tabs placed — boards are not connected to the panel.".into());
    }
    if cfg.sep == Separation::Mousebites {
        if cfg.mb_drill < 0.5 - ETOL {
            s.warnings.push("Mousebite drill < 0.5 mm: below many fabs' minimum NPTH size.".into());
        }
        if cfg.mb_spacing - cfg.mb_drill < 0.2 - ETOL {
            s.warnings.push("⚠ Mousebite web (pitch − drill) < 0.2 mm: holes may merge.".into());
        }
        if w < cfg.mb_spacing * 3.0 {
            s.warnings.push("Tab narrower than 3 hole pitches: very few holes per tab.".into());
        }
    }
    s
}

/// Ear-clipping triangulation for filling concave polygons. Returns index triples.
pub fn triangulate(p: &[Pt]) -> Vec<[usize; 3]> {
    let n = p.len();
    if n < 3 {
        return vec![];
    }
    let signed: f64 = (0..n).map(|i| {
        let (a, b) = (p[i], p[(i + 1) % n]);
        a.0 * b.1 - b.0 * a.1
    }).sum();
    let ccw = signed > 0.0;
    let mut idx: Vec<usize> = (0..n).collect();
    let mut tris = Vec::new();
    let cross = |a: Pt, b: Pt, c: Pt| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (p[ia], p[ib], p[ic]);
            let cr = cross(a, b, c);
            if (ccw && cr <= 0.0) || (!ccw && cr >= 0.0) {
                continue; // reflex
            }
            let inside = idx.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let q = p[j];
                let (d1, d2, d3) = (cross(a, b, q), cross(b, c, q), cross(c, a, q));
                if ccw { d1 > 0.0 && d2 > 0.0 && d3 > 0.0 } else { d1 < 0.0 && d2 < 0.0 && d3 < 0.0 }
            });
            if inside {
                continue;
            }
            tris.push([ia, ib, ic]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            break; // degenerate polygon
        }
    }
    if idx.len() == 3 {
        tris.push([idx[0], idx[1], idx[2]]);
    }
    tris
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_outline(w: f64, h: f64) -> Outline {
        Outline {
            loops: vec![vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]],
            open: vec![],
            min: (0.0, 0.0),
            max: (w, h),
            separate: 0,
        }
    }

    #[test]
    fn grid_and_rails() {
        let o = rect_outline(10.0, 20.0);
        let cfg = PanelConfig { rows: 1, cols: 2, hspace: 3.0, framing: Framing::RailsTB, rail_width: 5.0, rail_space: 2.0, ..Default::default() };
        let p = build(&o, &cfg);
        assert_eq!(p.boards.len(), 2);
        assert!((p.size.0 - 23.0).abs() < 1e-9);
        assert!((p.size.1 - (20.0 + 2.0 * 7.0)).abs() < 1e-9);
        assert_eq!(p.rails.len(), 2);
        assert_eq!(p.hgaps.len(), 1);
    }

    #[test]
    fn rotation_swaps_bbox() {
        let o = rect_outline(10.0, 20.0);
        let cfg = PanelConfig { rotation: 90, framing: Framing::None, cols: 1, ..Default::default() };
        let p = build(&o, &cfg);
        assert!((p.size.0 - 20.0).abs() < 1e-9 && (p.size.1 - 10.0).abs() < 1e-9);
    }

    #[test]
    fn min_size_frame() {
        let o = rect_outline(10.0, 20.0);
        let cfg = PanelConfig { framing: Framing::Frame, min_w: 70.0, min_h: 70.0, ..Default::default() };
        let p = build(&o, &cfg);
        assert!((p.size.0 - 70.0).abs() < 1e-9 && (p.size.1 - 70.0).abs() < 1e-9);
    }

    #[test]
    fn vcut_lines_2x1() {
        let o = rect_outline(10.0, 20.0);
        let cfg = PanelConfig { hspace: 0.0, rail_space: 0.0, sep: Separation::VCut, ..Default::default() };
        let p = build(&o, &cfg);
        let s = separation(&p, &cfg);
        assert_eq!(s.vcuts.iter().filter(|v| v.vertical).count(), 1);
        assert_eq!(s.vcuts.iter().filter(|v| !v.vertical).count(), 2);
        assert!(s.vcuts.iter().all(|v| !v.conflict));
    }

    #[test]
    fn tabs_2x1_rails() {
        let o = rect_outline(10.0, 20.0);
        let cfg = PanelConfig { hspace: 2.0, rail_space: 2.0, tab_width: 3.0, tabs_h: 1, tabs_v: 2, ..Default::default() };
        let p = build(&o, &cfg);
        let s = separation(&p, &cfg);
        assert_eq!(s.tabs.len(), 2 + 2 + 2); // between boards, top, bottom
        assert!(!s.holes.is_empty());
    }

    #[test]
    fn tab_on_narrow_neck() {
        // board with a narrow top neck (x 3..7 at y=0), shoulders at y=2
        let o = Outline {
            loops: vec![vec![(3.0, 0.0), (7.0, 0.0), (7.0, 2.0), (10.0, 2.0), (10.0, 20.0), (0.0, 20.0), (0.0, 2.0), (3.0, 2.0)]],
            open: vec![],
            min: (0.0, 0.0),
            max: (10.0, 20.0),
            separate: 0,
        };
        let cfg = PanelConfig { cols: 1, tab_width: 3.0, tabs_h: 1, ..Default::default() };
        let p = build(&o, &cfg);
        assert_eq!(p.sides[TOP], vec![(3.0, 7.0)]);
        let s = separation(&p, &cfg);
        let top = s.tabs.iter().find(|t| t.max.1 <= p.boards[0].bbox.min.1 + 1e-6).unwrap();
        assert!(top.min.0 >= p.boards[0].bbox.min.0 + 3.0 - 1e-6);
        assert!(!p.rectangular);
    }

    #[test]
    fn triangulate_concave() {
        // L shape
        let l = vec![(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)];
        assert_eq!(triangulate(&l).len(), 4);
    }
}

