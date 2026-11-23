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
    /// legacy single text (read from old files, converted to `texts`)
    #[serde(skip_serializing)]
    pub text: String,
    #[serde(skip_serializing)]
    pub text_size: f64,
    #[serde(skip_serializing)]
    pub text_pos: TextPos,
    // --- export ---
    pub vcut_layer: String,
    /// legacy (converted to `ref_template`)
    #[serde(skip_serializing)]
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

    // --- advanced layout ---
    /// rotate every other board by 180°
    pub alternation: Alternation,
    /// extra spacing of every second column / row gap (mm, may be negative)
    pub hspace_alt: f64,
    pub vspace_alt: f64,
    /// backbone bar width in the gaps between columns / rows (mm), 0 = none
    pub bone_cols: f64,
    pub bone_rows: f64,
    /// place a backbone in every Nth gap
    pub bone_every: u32,
    // --- advanced tabs / cuts ---
    /// tab width on left / right edges (tab_width is used for top / bottom)
    #[serde(default)]
    pub tab_width_lr: f64,
    pub tab_mode: TabMode,
    /// distance between tabs in spacing mode (mm)
    pub tab_spacing: f64,
    /// mousebite row is extended past the tab on both ends (mm)
    pub mb_prolong: f64,
    /// V-cut lines are drawn this far past the panel edge (mm)
    pub vcut_extend: f64,
    /// round inner corners of the outline with this radius (mm), 0 = off
    pub mill_radius: f64,
    // --- advanced frame ---
    /// maximum panel size (mm), 0 = off; only checked
    pub max_w: f64,
    pub max_h: f64,
    pub corner: CornerStyle,
    pub corner_size: f64,
    pub copper_fill: bool,
    pub copper_clearance: f64,
    // --- advanced tooling / fiducials ---
    /// distance of the center from the outer panel edge, across the rail (mm); 0 = centered on rail
    pub tool_edge: f64,
    pub fid_edge: f64,
    // --- texts ---
    pub texts: Vec<TextCfg>,
    // --- advanced export ---
    /// net name per copy: {n} = board number, {orig} = original name
    pub net_template: String,
    /// reference per copy: {n} = board number, {orig} = original reference
    pub ref_template: String,
    /// Edge.Cuts line width (mm)
    pub edge_width: f64,
    /// set the page size to fit the panel
    pub page_fit: bool,
    /// drill / place file origin and grid origin
    pub aux_origin: OriginCorner,
    /// 3D render in high quality (slow)
    pub render_high: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Alternation {
    None,
    Cols,
    Rows,
    RowsCols,
}

impl Alternation {
    pub const ALL: [Alternation; 4] = [Alternation::None, Alternation::Cols, Alternation::Rows, Alternation::RowsCols];
    pub fn label(self) -> &'static str {
        match self {
            Alternation::None => "None",
            Alternation::Cols => "Every 2nd column",
            Alternation::Rows => "Every 2nd row",
            Alternation::RowsCols => "Checkerboard",
        }
    }
    fn flipped(self, row: u32, col: u32) -> bool {
        match self {
            Alternation::None => false,
            Alternation::Cols => col % 2 == 1,
            Alternation::Rows => row % 2 == 1,
            Alternation::RowsCols => (row + col) % 2 == 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TabMode {
    Count,
    Spacing,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum CornerStyle {
    Square,
    Chamfer,
    Fillet,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum OriginCorner {
    None,
    TopLeft,
    BottomLeft,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TextRail {
    Top,
    Bottom,
    Left,
    Right,
}

impl TextRail {
    pub const ALL: [TextRail; 4] = [TextRail::Top, TextRail::Bottom, TextRail::Left, TextRail::Right];
    pub fn label(self) -> &'static str {
        match self {
            TextRail::Top => "Top rail",
            TextRail::Bottom => "Bottom rail",
            TextRail::Left => "Left rail",
            TextRail::Right => "Right rail",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Justify {
    Start,
    Center,
    End,
}

pub const MAX_TEXTS: usize = 4;
pub const TEXT_LAYERS: [&str; 6] = ["F.SilkS", "B.SilkS", "F.Cu", "B.Cu", "F.Mask", "B.Mask"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TextCfg {
    /// may contain {date} {board} {rows} {cols} {count} {width} {height}
    pub text: String,
    pub size: f64,
    pub rail: TextRail,
    pub layer: String,
    pub justify: Justify,
    /// shift along the rail (mm); for Start / End: distance from the rail end
    pub offset: f64,
    /// stroke thickness (mm), 0 = automatic
    pub thickness: f64,
}

impl Default for TextCfg {
    fn default() -> Self {
        Self { text: String::new(), size: 1.0, rail: TextRail::Bottom, layer: "F.SilkS".into(), justify: Justify::Center, offset: 0.0, thickness: 0.0 }
    }
}

/// fill in the {placeholders} of a panel text
pub fn expand_text(t: &str, board: &str, p: &Panel) -> String {
    t.replace("{date}", &today())
        .replace("{board}", board)
        .replace("{rows}", &p.rows.to_string())
        .replace("{cols}", &p.cols.to_string())
        .replace("{count}", &p.boards.len().to_string())
        .replace("{width}", &format!("{:.1}", p.size.0))
        .replace("{height}", &format!("{:.1}", p.size.1))
}

/// today's date (UTC) as YYYY-MM-DD
fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let z = secs.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// {n} = board number (1-based), {orig} = original name
pub fn apply_template(tpl: &str, n: usize, orig: &str) -> String {
    tpl.replace("{n}", &n.to_string()).replace("{orig}", orig)
}

impl PanelConfig {
    /// read settings, including files written by older versions
    pub fn from_json(s: &str) -> Result<Self, String> {
        let mut c: PanelConfig = serde_json::from_str(s).map_err(|e| e.to_string())?;
        c.migrate();
        Ok(c)
    }

    fn migrate(&mut self) {
        if self.tab_width_lr <= 0.0 {
            self.tab_width_lr = self.tab_width;
        }
        if !self.text.trim().is_empty() && self.texts.is_empty() {
            let side_only = !self.framing.has_tb() && self.framing.has_lr();
            let rail = match (self.text_pos, side_only) {
                (TextPos::TopRail, false) => TextRail::Top,
                (TextPos::BottomRail, false) => TextRail::Bottom,
                (TextPos::TopRail, true) => TextRail::Left,
                (TextPos::BottomRail, true) => TextRail::Right,
            };
            self.texts.push(TextCfg { text: self.text.clone(), size: self.text_size, rail, ..Default::default() });
        }
        self.text.clear();
        if self.rename_refs {
            if self.ref_template == "{orig}" {
                self.ref_template = "{orig}_{n}".into();
            }
            self.rename_refs = false;
        }
        self.texts.truncate(MAX_TEXTS);
        self.bone_every = self.bone_every.max(1);
    }
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
        let keep = (c.rows, c.cols, c.rotation, c.out_dir.clone(), c.out_postfix.clone(), c.copy_pro, c.open_after, c.settings_file.clone(), c.texts.clone());
        let keep2 = (c.net_template.clone(), c.ref_template.clone(), c.render_high);
        *c = d;
        (c.rows, c.cols, c.rotation, c.out_dir, c.out_postfix, c.copy_pro, c.open_after, c.settings_file, c.texts) = keep;
        (c.net_template, c.ref_template, c.render_high) = keep2;
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
            alternation: Alternation::None,
            hspace_alt: 0.0,
            vspace_alt: 0.0,
            bone_cols: 0.0,
            bone_rows: 0.0,
            bone_every: 1,
            tab_width_lr: 5.0,
            tab_mode: TabMode::Count,
            tab_spacing: 30.0,
            mb_prolong: 0.0,
            vcut_extend: 0.0,
            mill_radius: 0.0,
            max_w: 0.0,
            max_h: 0.0,
            corner: CornerStyle::Square,
            corner_size: 2.0,
            copper_fill: false,
            copper_clearance: 0.5,
            tool_edge: 0.0,
            fid_edge: 0.0,
            texts: Vec::new(),
            net_template: "Board_{n}-{orig}".into(),
            ref_template: "{orig}".into(),
            edge_width: 0.05,
            page_fit: false,
            aux_origin: OriginCorner::None,
            render_high: false,
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
    /// rotation of this copy in degrees (0/90/180/270)
    pub rotation: u32,
    /// per side: straight intervals on the bbox line, relative to bbox min (usable for tabs)
    pub sides: [Vec<(f64, f64)>; 4],
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
    /// outer outline is (nearly) a rectangle
    pub rectangular: bool,
    pub rot_center: Pt,
    /// actual rail widths (after min-size growth), 0 if absent
    pub rail_tb_w: f64,
    pub rail_lr_w: f64,
    /// width of the gap after column k / row k
    pub col_gaps: Vec<f64>,
    pub row_gaps: Vec<f64>,
    /// backbone bars, and whether gap k has one
    pub bones: Vec<RectMm>,
    pub col_bone: Vec<bool>,
    pub row_bone: Vec<bool>,
}

impl Panel {
    /// original board coordinate -> panel coordinate for board copy `i`
    pub fn to_panel(&self, i: usize, p: Pt) -> Pt {
        let r = rotate(p, self.rot_center, self.boards[i].rotation);
        let s = self.boards[i].shift;
        (r.0 + s.0, r.1 + s.1)
    }
}

impl Panel {
    /// widths of the milled slots: gaps between boards (two slots where a backbone sits) and gaps to the rails
    pub fn slots(&self, cfg: &PanelConfig) -> Vec<f64> {
        let cols = self.col_gaps.iter().zip(&self.col_bone).map(|(g, b)| if *b { (g - cfg.bone_cols) / 2.0 } else { *g });
        let rows = self.row_gaps.iter().zip(&self.row_bone).map(|(g, b)| if *b { (g - cfg.bone_rows) / 2.0 } else { *g });
        let mut v: Vec<f64> = cols.chain(rows).collect();
        if cfg.framing != Framing::None {
            v.push(cfg.rail_space);
        }
        v
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

    // rotated board, plus the 180° flipped variant used by alternation (same bbox for both)
    let rot_variant = |deg: u32| -> Vec<Vec<Pt>> {
        o.loops.iter().map(|l| l.iter().map(|&p| rotate(p, c, deg)).collect()).collect()
    };
    let rots = [cfg.rotation % 360, (cfg.rotation + 180) % 360];
    let variants = [rot_variant(rots[0]), rot_variant(rots[1])];
    let (mut mn, mut mx) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for &(x, y) in variants[0].iter().flatten() {
        mn = (mn.0.min(x), mn.1.min(y));
        mx = (mx.0.max(x), mx.1.max(y));
    }
    let (bw, bh) = (mx.0 - mn.0, mx.1 - mn.1);

    // gaps: every second gap may differ
    let gap = |base: f64, alt: f64, k: u32| (base + if k % 2 == 1 { alt } else { 0.0 }).max(0.0);
    let col_gaps: Vec<f64> = (0..cols - 1).map(|k| gap(cfg.hspace, cfg.hspace_alt, k)).collect();
    let row_gaps: Vec<f64> = (0..rows - 1).map(|k| gap(cfg.vspace, cfg.vspace_alt, k)).collect();
    // column / row origins relative to the board area
    let mut col_x = vec![0.0];
    for g in &col_gaps {
        col_x.push(col_x.last().unwrap() + bw + g);
    }
    let mut row_y = vec![0.0];
    for g in &row_gaps {
        row_y.push(row_y.last().unwrap() + bh + g);
    }
    let area_w = col_x.last().unwrap() + bw;
    let area_h = row_y.last().unwrap() + bh;

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

    let norm = |v: &Vec<Vec<Pt>>| -> Vec<Pt> {
        v.first().map(|l| l.iter().map(|&(x, y)| (x - mn.0, y - mn.1)).collect()).unwrap_or_default()
    };
    let var_sides = [side_intervals(&norm(&variants[0]), bw, bh), side_intervals(&norm(&variants[1]), bw, bh)];
    let mut boards = Vec::new();
    for r in 0..rows {
        for col in 0..cols {
            let v = cfg.alternation.flipped(r, col) as usize;
            let x0 = ox + col_x[col as usize];
            let y0 = oy + row_y[r as usize];
            let loops = variants[v]
                .iter()
                .map(|l| l.iter().map(|&(x, y)| (x - mn.0 + x0, y - mn.1 + y0)).collect())
                .collect();
            boards.push(Placed {
                loops,
                bbox: RectMm::new(x0, y0, x0 + bw, y0 + bh),
                row: r,
                col,
                shift: (x0 - mn.0, y0 - mn.1),
                rotation: rots[v],
                sides: var_sides[v].clone(),
            });
        }
    }

    let mut hgaps = Vec::new();
    for (k, g) in col_gaps.iter().enumerate() {
        let x = ox + col_x[k] + bw;
        hgaps.push(RectMm::new(x, oy, x + g, oy + area_h));
    }
    let mut vgaps = Vec::new();
    for (k, g) in row_gaps.iter().enumerate() {
        let y = oy + row_y[k] + bh;
        vgaps.push(RectMm::new(ox, y, ox + area_w, y + g));
    }

    // backbones: a bar in the middle of a gap, boards are tabbed to it. Not for V-cut.
    let every = cfg.bone_every.max(1) as usize;
    let has_bone = |w: f64, g: f64, k: usize| cfg.sep != Separation::VCut && w > ETOL && g > w + ETOL && k % every == 0;
    let col_bone: Vec<bool> = col_gaps.iter().enumerate().map(|(k, &g)| has_bone(cfg.bone_cols, g, k)).collect();
    let row_bone: Vec<bool> = row_gaps.iter().enumerate().map(|(k, &g)| has_bone(cfg.bone_rows, g, k)).collect();
    let mut bones = Vec::new();
    // bars run up to the rails if there are any, so they are held on both ends
    let (by0, by1) = if tb { (rail_tb, panel_h - rail_tb) } else { (oy, oy + area_h) };
    let (bx0, bx1) = if lr { (rail_lr, panel_w - rail_lr) } else { (ox, ox + area_w) };
    for (k, r) in hgaps.iter().enumerate() {
        if col_bone[k] {
            let m = (r.min.0 + r.max.0) / 2.0;
            bones.push(RectMm::new(m - cfg.bone_cols / 2.0, by0, m + cfg.bone_cols / 2.0, by1));
        }
    }
    for (k, r) in vgaps.iter().enumerate() {
        if row_bone[k] {
            let m = (r.min.1 + r.max.1) / 2.0;
            bones.push(RectMm::new(bx0, m - cfg.bone_rows / 2.0, bx1, m + cfg.bone_rows / 2.0));
        }
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

    let rectangular = polygon_area(&norm(&variants[0])) > bw * bh * 0.999;

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
        rectangular,
        rot_center: c,
        rail_tb_w: if tb { rail_tb } else { 0.0 },
        rail_lr_w: if lr { rail_lr } else { 0.0 },
        col_gaps,
        row_gaps,
        bones,
        col_bone,
        row_bone,
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
    /// anchor point (see `justify`)
    pub pos: Pt,
    pub text: String,
    pub size: f64,
    /// estimated extents for checks / preview
    pub rect: RectMm,
    /// 0 = horizontal, 90 = reads bottom to top (side rails)
    pub angle: f64,
    pub layer: String,
    pub justify: Justify,
    pub thickness: f64,
}

pub struct Features {
    pub items: Vec<Feature>,
    pub texts: Vec<PanelText>,
    /// copper fill pieces on the rails (F.Cu + B.Cu)
    pub copper: Vec<RectMm>,
    pub warnings: Vec<String>,
    /// a tooling hole / a fiducial is off the rail, on a V-cut or overlaps something
    pub tool_bad: bool,
    pub fid_bad: bool,
    /// indices (into the settings) of texts that do not fit or cover something
    pub text_bad: Vec<usize>,
}

/// `edge` = distance from the outer panel edge across the rail, 0 = centered on the rail
fn corner_pos(p: &Panel, cfg: &PanelConfig, corner: usize, along: f64, edge: f64) -> Pt {
    let (pw, ph) = p.size;
    let right = corner == 1 || corner == 3;
    let bottom = corner == 2 || corner == 3;
    let across = |rail_w: f64| if edge > ETOL { edge } else { rail_w / 2.0 };
    if cfg.framing.has_tb() {
        let x = if right { pw - along } else { along };
        let a = across(p.rail_tb_w);
        (x, if bottom { ph - a } else { a })
    } else if cfg.framing.has_lr() {
        let a = across(p.rail_lr_w);
        let y = if bottom { ph - along } else { along };
        (if right { pw - a } else { a }, y)
    } else {
        (if right { pw - along } else { along }, if bottom { ph - along } else { along })
    }
}

const CORNER_NAMES: [&str; 4] = ["top-left", "top-right", "bottom-left", "bottom-right"];

/// remove `blocked` intervals from (lo, hi); pieces shorter than `min_len` are dropped
fn subtract(lo: f64, hi: f64, blocked: Vec<(f64, f64)>, min_len: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut cur = lo;
    for (a, b) in merge(blocked) {
        if a > cur && a.min(hi) - cur >= min_len {
            out.push((cur, a.min(hi)));
        }
        cur = cur.max(b);
        if cur >= hi {
            break;
        }
    }
    if hi - cur >= min_len {
        out.push((cur, hi));
    }
    out
}

/// solid copper on the rails, kept clear of the panel edge, holes, fiducials, V-cuts and copper text
fn copper_rects(p: &Panel, cfg: &PanelConfig, items: &[Feature], texts: &[PanelText], vcuts: &[VCutLine]) -> Vec<RectMm> {
    let cl = cfg.copper_clearance.max(0.0);
    let mut out = Vec::new();
    for rail in &p.rails {
        let r = RectMm::new(rail.min.0 + cl, rail.min.1 + cl, rail.max.0 - cl, rail.max.1 - cl);
        if rail.w() <= 2.0 * cl + 0.2 || rail.h() <= 2.0 * cl + 0.2 {
            continue;
        }
        let horiz = r.w() >= r.h();
        // along = the long axis of the rail
        let (lo, hi, c0, c1) = if horiz { (r.min.0, r.max.0, r.min.1, r.max.1) } else { (r.min.1, r.max.1, r.min.0, r.max.0) };
        let mut blocked: Vec<(f64, f64)> = Vec::new();
        for f in items {
            let (a, c) = if horiz { f.pos } else { (f.pos.1, f.pos.0) };
            if c + f.r + cl > c0 && c - f.r - cl < c1 {
                blocked.push((a - f.r - cl, a + f.r + cl));
            }
        }
        for v in vcuts.iter().filter(|v| v.vertical == horiz) {
            blocked.push((v.pos - cl.max(0.3), v.pos + cl.max(0.3)));
        }
        // chamfered / rounded panel corners: keep out of the corner squares
        if cfg.corner != CornerStyle::Square {
            let s = cfg.corner_size + cl;
            let (pa, pc) = if horiz { p.size } else { (p.size.1, p.size.0) };
            for (a0, a1) in [(0.0, s), (pa - s, pa)] {
                for (k0, k1) in [(0.0, s), (pc - s, pc)] {
                    if k1 > c0 && k0 < c1 {
                        blocked.push((a0, a1));
                    }
                }
            }
        }
        for t in texts.iter().filter(|t| t.layer.ends_with(".Cu")) {
            let (a0, a1, t0, t1) = if horiz { (t.rect.min.0, t.rect.max.0, t.rect.min.1, t.rect.max.1) } else { (t.rect.min.1, t.rect.max.1, t.rect.min.0, t.rect.max.0) };
            if t1 > c0 && t0 < c1 {
                blocked.push((a0 - cl, a1 + cl));
            }
        }
        for (a, b) in subtract(lo, hi, blocked, 1.0) {
            out.push(if horiz { RectMm::new(a, c0, b, c1) } else { RectMm::new(c0, a, c1, b) });
        }
    }
    out
}

/// `board` = board name, for the {board} placeholder in texts
pub fn features(p: &Panel, cfg: &PanelConfig, vcuts: &[VCutLine], board: &str) -> Features {
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let (mut tool_bad, mut fid_bad) = (false, false);
    let mut text_bad: Vec<usize> = Vec::new();
    for corner in 0..cfg.tooling.count() {
        items.push(Feature {
            kind: FeatureKind::Tooling,
            pos: corner_pos(p, cfg, corner, cfg.tool_along, cfg.tool_edge),
            r: cfg.tool_d / 2.0,
            corner: CORNER_NAMES[corner],
        });
    }
    for corner in 0..cfg.fiducials.count() {
        let pos = corner_pos(p, cfg, corner, cfg.fid_along, cfg.fid_edge);
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
            *(if f.kind == FeatureKind::Tooling { &mut tool_bad } else { &mut fid_bad }) = true;
            warnings.push(format!(
                "⚠ {} {} does not fit on a rail (Ø {:.2} mm). Use rails, make them wider, or change the corner distance.",
                kind_name(f.kind), f.corner, 2.0 * f.r
            ));
        }
        for v in vcuts {
            let d = if v.vertical { (f.pos.0 - v.pos).abs() } else { (f.pos.1 - v.pos).abs() };
            if d < f.r + 0.3 {
                *(if f.kind == FeatureKind::Tooling { &mut tool_bad } else { &mut fid_bad }) = true;
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
                // two holes: move the holes; anything with a fiducial: move the fiducial
                let both_holes = a.kind == FeatureKind::Tooling && b.kind == FeatureKind::Tooling;
                *(if both_holes { &mut tool_bad } else { &mut fid_bad }) = true;
                warnings.push(format!(
                    "⚠ {} {} overlaps {} {} — increase the corner distance of one of them.",
                    kind_name(a.kind), a.corner, kind_name(b.kind).to_lowercase(), b.corner
                ));
            }
        }
    }

    // texts on the rails
    let mut texts: Vec<PanelText> = Vec::new();
    let (pw, ph) = p.size;
    for (i, tc) in cfg.texts.iter().take(MAX_TEXTS).enumerate() {
        let t = expand_text(tc.text.trim(), board, p);
        if t.is_empty() {
            continue;
        }
        let no = i + 1;
        let (tb, lr) = (cfg.framing.has_tb(), cfg.framing.has_lr());
        let rail = match tc.rail {
            TextRail::Top if tb => Some(RectMm::new(0.0, 0.0, pw, p.rail_tb_w)),
            TextRail::Bottom if tb => Some(RectMm::new(0.0, ph - p.rail_tb_w, pw, ph)),
            TextRail::Left if lr => Some(RectMm::new(0.0, 0.0, p.rail_lr_w, ph)),
            TextRail::Right if lr => Some(RectMm::new(pw - p.rail_lr_w, 0.0, pw, ph)),
            _ => None,
        };
        let Some(rail) = rail else {
            text_bad.push(i);
            warnings.push(format!("⚠ Text {no}: the panel has no {} — choose another rail or change the frame.", tc.rail.label().to_lowercase()));
            continue;
        };
        // rough KiCad stroke font width: ~0.9 × size per char
        let w = t.chars().count() as f64 * tc.size * 0.9;
        let h = tc.size;
        let vertical = matches!(tc.rail, TextRail::Left | TextRail::Right);
        let (pos, rect) = if !vertical {
            let y = (rail.min.1 + rail.max.1) / 2.0;
            let (x, x0) = match tc.justify {
                Justify::Start => (rail.min.0 + tc.offset, rail.min.0 + tc.offset),
                Justify::Center => (pw / 2.0 + tc.offset, pw / 2.0 + tc.offset - w / 2.0),
                Justify::End => (rail.max.0 - tc.offset, rail.max.0 - tc.offset - w),
            };
            ((x, y), RectMm::new(x0, y - h / 2.0, x0 + w, y + h / 2.0))
        } else {
            // reads bottom to top: start = lower end
            let x = (rail.min.0 + rail.max.0) / 2.0;
            let (y, y0) = match tc.justify {
                Justify::Start => (rail.max.1 - tc.offset, rail.max.1 - tc.offset - w),
                Justify::Center => (ph / 2.0 - tc.offset, ph / 2.0 - tc.offset - w / 2.0),
                Justify::End => (rail.min.1 + tc.offset, rail.min.1 + tc.offset),
            };
            ((x, y), RectMm::new(x - h / 2.0, y0, x + h / 2.0, y0 + w))
        };
        if rect.min.0 < rail.min.0 - ETOL || rect.max.0 > rail.max.0 + ETOL || rect.min.1 < rail.min.1 - ETOL || rect.max.1 > rail.max.1 + ETOL {
            text_bad.push(i);
            warnings.push(format!("⚠ Text {no} does not fit on the rail (too long / too big)."));
        }
        if items.iter().any(|f| {
            f.pos.0 + f.r > rect.min.0 && f.pos.0 - f.r < rect.max.0 && f.pos.1 + f.r > rect.min.1 && f.pos.1 - f.r < rect.max.1
        }) {
            text_bad.push(i);
            warnings.push(format!("⚠ Text {no} overlaps a tooling hole or fiducial."));
        }
        if texts.iter().any(|o| o.layer == tc.layer && o.rect.max.0 > rect.min.0 && o.rect.min.0 < rect.max.0 && o.rect.max.1 > rect.min.1 && o.rect.min.1 < rect.max.1) {
            text_bad.push(i);
            warnings.push(format!("⚠ Text {no} overlaps another text."));
        }
        texts.push(PanelText {
            pos,
            text: t,
            size: tc.size,
            rect,
            angle: if vertical { 90.0 } else { 0.0 },
            layer: tc.layer.clone(),
            justify: tc.justify,
            thickness: tc.thickness,
        });
    }
    if cfg.tooling != Corners::None && cfg.framing == Framing::None {
        warnings.push("Tooling holes / fiducials need rails or a frame.".into());
    }

    let copper = if cfg.copper_fill { copper_rects(p, cfg, &items, &texts, vcuts) } else { Vec::new() };
    if cfg.copper_fill && cfg.framing == Framing::None {
        warnings.push("Copper fill needs rails or a frame.".into());
    }

    // general checks
    if cfg.max_w > ETOL && pw > cfg.max_w + ETOL {
        warnings.push(format!("⚠ Panel is {pw:.1} mm wide — more than the maximum of {:.1} mm.", cfg.max_w));
    }
    if cfg.max_h > ETOL && ph > cfg.max_h + ETOL {
        warnings.push(format!("⚠ Panel is {ph:.1} mm high — more than the maximum of {:.1} mm.", cfg.max_h));
    }
    if p.boards.len() > 1 && !cfg.net_template.contains("{n}") {
        warnings.push("Net name pattern has no {n}: the same nets of all boards are joined into one net.".into());
    }
    if !cfg.net_template.contains("{orig}") {
        warnings.push("⚠ Net name pattern has no {orig}: all nets of a board would get the same name.".into());
    }
    if cfg.sep != Separation::VCut {
        for (w, name, gaps) in [(cfg.bone_cols, "columns", &p.col_gaps), (cfg.bone_rows, "rows", &p.row_gaps)] {
            if w > ETOL && gaps.iter().any(|&g| g <= w + ETOL) {
                warnings.push(format!("⚠ Backbone between {name} ({w:.2} mm) does not fit into the gap — increase the spacing."));
            }
        }
    } else if cfg.bone_cols > ETOL || cfg.bone_rows > ETOL {
        warnings.push("Backbones are not used with V-cut.".into());
    }
    if cfg.corner != CornerStyle::Square && cfg.framing != Framing::None {
        let limit = [p.rail_tb_w, p.rail_lr_w].into_iter().filter(|w| *w > ETOL).fold(f64::MAX, f64::min);
        if cfg.framing != Framing::Frame && cfg.corner_size > limit + ETOL {
            warnings.push("⚠ Panel corner size is larger than the rail width — it would cut into the board area.".into());
        }
    }
    text_bad.dedup();
    Features { items, texts, copper, warnings, tool_bad, fid_bad, text_bad }
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
        // far tighter than ETOL: the first segment of a rounded corner leaves the edge line by
        // less than a micrometre, and a tab reaching onto it leaves a hair-thin notch in the outline
        let on = |v: f64, t: f64| (v - t).abs() < 1e-6;
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
    /// tabs that did not fit: the shortest straight length they were asked to sit on,
    /// for top/bottom edges and for left/right edges
    pub short_h: Option<f64>,
    pub short_v: Option<f64>,
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
    let any_gap = p.col_gaps.iter().chain(p.row_gaps.iter()).any(|&g| g > ETOL);
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

/// tabs on one side of a board, bridging `len` mm outwards
struct EdgeTabs<'a> {
    board: &'a Placed,
    board_no: usize,
    side: usize,
    len: f64,
    usable: Vec<(f64, f64)>,
    /// the far end touches another board: cut there too
    far_cut: bool,
    target: &'static str,
}

fn tabs(p: &Panel, cfg: &PanelConfig) -> Sep {
    let mut s = Sep::default();
    let (bw, bh) = p.board_size;
    let idx = |r: u32, c: u32| (r * p.cols + c) as usize;
    let side_name = ["top", "right", "bottom", "left"];
    let longest = |v: &[(f64, f64)]| v.iter().map(|(a, b)| b - a).fold(0.0_f64, f64::max);
    let mut missing: Vec<String> = Vec::new();
    let mut too_narrow: Vec<f64> = Vec::new();

    let mut place = |s: &mut Sep, e: EdgeTabs| {
        if e.len <= ETOL {
            return;
        }
        let horiz_edge = e.side == TOP || e.side == BOTTOM;
        let (w, span, count) = if horiz_edge { (cfg.tab_width, bw, cfg.tabs_h) } else { (cfg.tab_width_lr, bh, cfg.tabs_v) };
        let n = match cfg.tab_mode {
            TabMode::Count => count,
            TabMode::Spacing => ((span / cfg.tab_spacing.max(0.5)).round() as u32).max(1),
        };
        if n == 0 {
            return;
        }
        let cs = tab_centers((0.0, span), &e.usable, n, w);
        if cs.is_empty() {
            let len = longest(&e.usable);
            let short = if horiz_edge { &mut s.short_h } else { &mut s.short_v };
            *short = Some(short.map_or(len, |v: f64| v.min(len)));
            too_narrow.push(w);
            missing.push(format!("board {} {} to {}: longest straight {:.2} mm", e.board_no, side_name[e.side], e.target, longest(&e.usable)));
        }
        let (x0, y0) = e.board.bbox.min;
        let (x1, y1) = e.board.bbox.max;
        for c in cs {
            // near = on this board's edge, far = `len` further out; `into` points into the board that is cut
            let (rect, near, far): (RectMm, (Pt, Pt, Pt), (Pt, Pt, Pt)) = match e.side {
                RIGHT => {
                    let (a, b) = (y0 + c - w / 2.0, y0 + c + w / 2.0);
                    (RectMm::new(x1, a, x1 + e.len, b), ((x1, a), (x1, b), (-1.0, 0.0)), ((x1 + e.len, a), (x1 + e.len, b), (1.0, 0.0)))
                }
                LEFT => {
                    let (a, b) = (y0 + c - w / 2.0, y0 + c + w / 2.0);
                    (RectMm::new(x0 - e.len, a, x0, b), ((x0, a), (x0, b), (1.0, 0.0)), ((x0 - e.len, a), (x0 - e.len, b), (-1.0, 0.0)))
                }
                BOTTOM => {
                    let (a, b) = (x0 + c - w / 2.0, x0 + c + w / 2.0);
                    (RectMm::new(a, y1, b, y1 + e.len), ((a, y1), (b, y1), (0.0, -1.0)), ((a, y1 + e.len), (b, y1 + e.len), (0.0, 1.0)))
                }
                _ => {
                    let (a, b) = (x0 + c - w / 2.0, x0 + c + w / 2.0);
                    (RectMm::new(a, y0 - e.len, b, y0), ((a, y0), (b, y0), (0.0, 1.0)), ((a, y0 - e.len), (b, y0 - e.len), (0.0, -1.0)))
                }
            };
            s.tabs.push(rect);
            let mut cuts = vec![near];
            if e.far_cut {
                cuts.push(far);
            }
            for (a, b, into) in cuts {
                match cfg.sep {
                    Separation::Mousebites => {
                        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                        let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
                        // the hole row may run past the tab on both ends
                        let pro = cfg.mb_prolong.max(0.0);
                        let (a, len) = ((a.0 - ux * pro, a.1 - uy * pro), len + 2.0 * pro);
                        let n = ((len - cfg.mb_drill) / cfg.mb_spacing).floor().max(0.0) as usize + 1;
                        let start = (len - (n - 1) as f64 * cfg.mb_spacing) / 2.0;
                        for i in 0..n {
                            let t = start + i as f64 * cfg.mb_spacing;
                            s.holes.push(((a.0 + ux * t + into.0 * cfg.mb_offset, a.1 + uy * t + into.1 * cfg.mb_offset), cfg.mb_drill));
                        }
                    }
                    _ => s.breaks.push((a, b)),
                }
            }
        }
    };

    for (i, b) in p.boards.iter().enumerate() {
        let no = i + 1;
        let (tb, lr) = (cfg.framing.has_tb(), cfg.framing.has_lr());
        // ---- right: neighbour board (direct or via backbone) or right rail
        if b.col + 1 < p.cols {
            let n = &p.boards[idx(b.row, b.col + 1)];
            let gap = p.col_gaps[b.col as usize];
            if p.col_bone[b.col as usize] {
                let half = (gap - cfg.bone_cols) / 2.0;
                place(&mut s, EdgeTabs { board: b, board_no: no, side: RIGHT, len: half, usable: b.sides[RIGHT].clone(), far_cut: false, target: "backbone" });
                place(&mut s, EdgeTabs { board: n, board_no: no + 1, side: LEFT, len: half, usable: n.sides[LEFT].clone(), far_cut: false, target: "backbone" });
            } else {
                place(&mut s, EdgeTabs { board: b, board_no: no, side: RIGHT, len: gap, usable: intersect(&b.sides[RIGHT], &n.sides[LEFT]), far_cut: true, target: "next board" });
            }
        } else if lr {
            place(&mut s, EdgeTabs { board: b, board_no: no, side: RIGHT, len: cfg.rail_space, usable: b.sides[RIGHT].clone(), far_cut: false, target: "rail" });
        }
        // ---- left rail
        if b.col == 0 && lr {
            place(&mut s, EdgeTabs { board: b, board_no: no, side: LEFT, len: cfg.rail_space, usable: b.sides[LEFT].clone(), far_cut: false, target: "rail" });
        }
        // ---- bottom: neighbour board (direct or via backbone) or bottom rail
        if b.row + 1 < p.rows {
            let ni = idx(b.row + 1, b.col);
            let n = &p.boards[ni];
            let gap = p.row_gaps[b.row as usize];
            if p.row_bone[b.row as usize] {
                let half = (gap - cfg.bone_rows) / 2.0;
                place(&mut s, EdgeTabs { board: b, board_no: no, side: BOTTOM, len: half, usable: b.sides[BOTTOM].clone(), far_cut: false, target: "backbone" });
                place(&mut s, EdgeTabs { board: n, board_no: ni + 1, side: TOP, len: half, usable: n.sides[TOP].clone(), far_cut: false, target: "backbone" });
            } else {
                place(&mut s, EdgeTabs { board: b, board_no: no, side: BOTTOM, len: gap, usable: intersect(&b.sides[BOTTOM], &n.sides[TOP]), far_cut: true, target: "next board" });
            }
        } else if tb {
            place(&mut s, EdgeTabs { board: b, board_no: no, side: BOTTOM, len: cfg.rail_space, usable: b.sides[BOTTOM].clone(), far_cut: false, target: "rail" });
        }
        // ---- top rail
        if b.row == 0 && tb {
            place(&mut s, EdgeTabs { board: b, board_no: no, side: TOP, len: cfg.rail_space, usable: b.sides[TOP].clone(), far_cut: false, target: "rail" });
        }
    }

    // ---- rules
    let slots = p.slots(cfg);
    if slots.iter().any(|g| *g <= ETOL) {
        s.warnings.push("⚠ A spacing is 0 mm: no room for tabs there. Use ≥ router Ø, or switch to V-cut.".into());
    }
    let min_gap = slots.iter().copied().filter(|g| *g > ETOL).fold(f64::MAX, f64::min);
    if min_gap < cfg.mill_d - ETOL {
        s.warnings.push(format!("⚠ Gap {min_gap:.2} mm is smaller than the router Ø {:.2} mm — the fab cannot mill between the boards.", cfg.mill_d));
    }
    if !missing.is_empty() {
        let w = too_narrow.iter().copied().fold(0.0_f64, f64::max);
        s.warnings.push(format!(
            "⚠ No straight section ≥ {w:.2} mm (tab width) on {} edge(s) — no tab placed there:\n  • {}\nReduce tab width below the longest straight length.",
            missing.len(),
            missing.join("\n  • ")
        ));
    }
    if s.tabs.is_empty() && (cfg.framing != Framing::None || p.boards.len() > 1) {
        s.warnings.push("⚠ No tabs placed — boards are not connected to the panel.".into());
    }
    if cfg.sep == Separation::Mousebites {
        if cfg.mb_drill < 0.5 - ETOL {
            s.warnings.push("Mousebite drill < 0.5 mm: below many fabs' minimum NPTH size.".into());
        }
        if cfg.mb_spacing - cfg.mb_drill < 0.2 - ETOL {
            s.warnings.push("⚠ Mousebite web (pitch − drill) < 0.2 mm: holes may merge.".into());
        }
        if cfg.tab_width.min(cfg.tab_width_lr) < cfg.mb_spacing * 3.0 {
            s.warnings.push("Tab narrower than 3 hole pitches: very few holes per tab.".into());
        }
    }
    s
}

// Ear-clipping triangulation for filling concave polygons. Returns index triples.
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
        assert_eq!(p.boards[0].sides[TOP], vec![(3.0, 7.0)]);
        let s = separation(&p, &cfg);
        let top = s.tabs.iter().find(|t| t.max.1 <= p.boards[0].bbox.min.1 + 1e-6).unwrap();
        assert!(top.min.0 >= p.boards[0].bbox.min.0 + 3.0 - 1e-6);
        assert!(!p.rectangular);
    }

    #[test]
    fn alternation_flips_every_second_column() {
        // neck on top: flipped boards have it at the bottom
        let o = Outline {
            loops: vec![vec![(3.0, 0.0), (7.0, 0.0), (7.0, 2.0), (10.0, 2.0), (10.0, 20.0), (0.0, 20.0), (0.0, 2.0), (3.0, 2.0)]],
            open: vec![],
            min: (0.0, 0.0),
            max: (10.0, 20.0),
            separate: 0,
        };
        let cfg = PanelConfig { cols: 2, alternation: Alternation::Cols, ..Default::default() };
        let p = build(&o, &cfg);
        assert_eq!((p.boards[0].rotation, p.boards[1].rotation), (0, 180));
        assert_eq!(p.boards[1].sides[BOTTOM], vec![(3.0, 7.0)]);
        assert_eq!(p.boards[1].sides[TOP], vec![(0.0, 10.0)]);
        // a point on the neck of the original board lands at the bottom of the flipped copy
        let q = p.to_panel(1, (5.0, 0.0));
        assert!((q.1 - p.boards[1].bbox.max.1).abs() < 1e-9);
    }

    #[test]
    fn uneven_gaps() {
        let o = rect_outline(10.0, 20.0);
        let cfg = PanelConfig { cols: 4, hspace: 2.0, hspace_alt: 3.0, framing: Framing::None, ..Default::default() };
        let p = build(&o, &cfg);
        assert_eq!(p.col_gaps, vec![2.0, 5.0, 2.0]);
        assert!((p.size.0 - (40.0 + 9.0)).abs() < 1e-9);
        assert!((p.boards[2].bbox.min.0 - 27.0).abs() < 1e-9);
    }

    #[test]
    fn backbone_between_columns() {
        let o = rect_outline(10.0, 20.0);
        let cfg = PanelConfig { cols: 2, hspace: 8.0, bone_cols: 3.0, tabs_v: 2, tabs_h: 1, ..Default::default() };
        let p = build(&o, &cfg);
        assert_eq!(p.bones.len(), 1);
        assert_eq!(p.col_bone, vec![true]);
        // bar is centered in the gap and reaches both rails
        let b = p.bones[0];
        assert!((b.min.0 - 12.5).abs() < 1e-9 && (b.max.0 - 15.5).abs() < 1e-9);
        assert!((b.min.1 - p.rail_tb_w).abs() < 1e-9);
        let s = separation(&p, &cfg);
        // per board: 2 tabs to the backbone + top + bottom
        assert_eq!(s.tabs.len(), 2 * (2 + 1 + 1));
        // tabs to the backbone are 2.5 mm long and end at the bar
        assert!(s.tabs.iter().any(|t| (t.min.0 - 10.0).abs() < 1e-9 && (t.max.0 - 12.5).abs() < 1e-9));
        assert!(s.tabs.iter().any(|t| (t.min.0 - 15.5).abs() < 1e-9 && (t.max.0 - 18.0).abs() < 1e-9));
        // V-cut never gets a backbone
        let v = PanelConfig { sep: Separation::VCut, ..cfg };
        assert!(build(&o, &v).bones.is_empty());
    }

    #[test]
    fn tabs_by_spacing() {
        let o = rect_outline(100.0, 20.0);
        let cfg = PanelConfig { cols: 1, tab_mode: TabMode::Spacing, tab_spacing: 25.0, tab_width: 3.0, tab_width_lr: 3.0, ..Default::default() };
        let p = build(&o, &cfg);
        let s = separation(&p, &cfg);
        // 100 mm edge / 25 mm = 4 tabs on top and 4 on the bottom
        assert_eq!(s.tabs.len(), 8);
    }

    #[test]
    fn mousebites_prolonged() {
        let o = rect_outline(30.0, 20.0);
        let base = PanelConfig { cols: 1, tab_width: 4.0, ..Default::default() };
        let p = build(&o, &base);
        let n0 = separation(&p, &base).holes.len();
        let long = PanelConfig { mb_prolong: 1.6, ..base };
        assert!(separation(&p, &long).holes.len() > n0);
    }

    #[test]
    fn texts_and_copper() {
        let o = rect_outline(40.0, 20.0);
        let cfg = PanelConfig {
            cols: 2,
            framing: Framing::Frame,
            copper_fill: true,
            texts: vec![
                TextCfg { text: "{board} {cols}x{rows}".into(), rail: TextRail::Top, ..Default::default() },
                TextCfg { text: "SIDE".into(), rail: TextRail::Left, justify: Justify::Start, offset: 12.0, ..Default::default() },
            ],
            ..Default::default()
        };
        let p = build(&o, &cfg);
        let s = separation(&p, &cfg);
        let f = features(&p, &cfg, &s.vcuts, "demo");
        assert_eq!(f.texts[0].text, "demo 2x1");
        assert_eq!(f.texts[1].angle, 90.0);
        // side text sits on the left rail
        assert!(f.texts[1].rect.max.0 <= p.rail_lr_w + 1e-9);
        assert!(f.warnings.iter().all(|w| !w.starts_with('⚠')), "{:?}", f.warnings);
        // copper stays clear of every hole / fiducial
        assert!(!f.copper.is_empty());
        for c in &f.copper {
            for it in &f.items {
                let dx = (it.pos.0 - it.pos.0.clamp(c.min.0, c.max.0)).abs();
                let dy = (it.pos.1 - it.pos.1.clamp(c.min.1, c.max.1)).abs();
                assert!((dx * dx + dy * dy).sqrt() >= it.r + cfg.copper_clearance - 1e-9);
            }
        }
        // a text on a rail that does not exist is reported, not placed
        let no_rail = PanelConfig { framing: Framing::RailsTB, ..cfg };
        let p2 = build(&o, &no_rail);
        let f2 = features(&p2, &no_rail, &[], "demo");
        assert_eq!(f2.texts.len(), 1);
        assert!(f2.warnings.iter().any(|w| w.contains("Text 2")));
    }

    #[test]
    fn old_settings_file_is_converted() {
        let old = r#"{"rows":2,"cols":3,"tab_width":3.5,"text":"REV A","text_size":1.5,"text_pos":"TopRail","rename_refs":true,"framing":"RailsTB"}"#;
        let c = PanelConfig::from_json(old).unwrap();
        assert_eq!((c.rows, c.cols), (2, 3));
        assert_eq!(c.tab_width_lr, 3.5);
        assert_eq!(c.texts.len(), 1);
        assert_eq!((c.texts[0].text.as_str(), c.texts[0].size, c.texts[0].rail), ("REV A", 1.5, TextRail::Top));
        assert_eq!(c.ref_template, "{orig}_{n}");
        // written again without the old keys, and stable when read back
        let j = serde_json::to_string(&c).unwrap();
        assert!(!j.contains("rename_refs") && !j.contains("text_pos"));
        let c2 = PanelConfig::from_json(&j).unwrap();
        assert_eq!(serde_json::to_string(&c2).unwrap(), j);
    }

    #[test]
    fn name_patterns() {
        assert_eq!(apply_template("Board_{n}-{orig}", 2, "GND"), "Board_2-GND");
        assert_eq!(apply_template("{orig}_{n}", 3, "R1"), "R1_3");
        assert_eq!(apply_template("{orig}", 3, "R1"), "R1");
    }

    #[test]
    fn triangulate_concave() {
        // L shape
        let l = vec![(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)];
        assert_eq!(triangulate(&l).len(), 4);
    }
}

