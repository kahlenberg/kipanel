#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod board;
mod export;
mod panel;
mod sexpr;

use board::Outline;
use panel::{Corners, FeatureKind, FidSide, Framing, PanelConfig, Preset, RectMm, Separation, TextPos};
use std::path::PathBuf;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use kicad_ipc_rs::KiCadClientBlocking;

/// actions requested from inside the settings UI, executed after it (borrow rules)
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    BrowseOutDir,
    BrowseSettings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hl {
    None,
    HSpace,
    VSpace,
    Grid,
    Rails,
    RailGap,
    Tabs,
    Holes,
    VCuts,
    Mill,
    Tooling,
    Fiducials,
    Text,
}

struct App {
    status: String,
    outline: Option<Outline>,
    /// board S-expression as received from KiCad (used for export)
    source: Option<String>,
    board_stem: String,
    project_dir: Option<PathBuf>,
    pcbnew: Option<String>,
    notice: Notice,
    /// settings as last loaded / saved (JSON), to show unsaved changes
    saved_json: Option<String>,
    last_export: Option<PathBuf>,
    pending: Option<Pending>,
    cfg: PanelConfig,
    hl: Hl,
    warnings: Vec<String>,
    // view: screen = (mm - panel_center) * zoom + rect_center + pan
    zoom: f32,
    pan: Vec2,
    fit_pending: bool,
}

/// message in the status bar, optionally with a file the user can jump to
#[derive(Default)]
struct Notice {
    text: String,
    path: Option<PathBuf>,
    error: bool,
}

impl Notice {
    fn ok(text: impl Into<String>) -> Self {
        Notice { text: text.into(), path: None, error: false }
    }
    fn ok_path(text: impl Into<String>, path: PathBuf) -> Self {
        Notice { text: text.into(), path: Some(path), error: false }
    }
    fn err(text: impl Into<String>) -> Self {
        Notice { text: text.into(), path: None, error: true }
    }
}

struct BoardInfo {
    status: String,
    outline: Outline,
    text: String,
    stem: String,
    project_dir: Option<PathBuf>,
    pcbnew: Option<String>,
}

/// standalone mode: `kipanel <file.kicad_pcb>` (no KiCad needed, for testing)
fn load_board_file(path: &std::path::Path) -> Result<BoardInfo, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let root = sexpr::parse(&text)?;
    let outline = board::extract_outline(&root)?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "board".into());
    let status = format!("File mode  ·  {stem}  ·  board {:.2} × {:.2} mm", outline.width(), outline.height());
    Ok(BoardInfo { status, outline, text, stem, project_dir: path.parent().map(|p| p.to_path_buf()), pcbnew: None })
}

fn load_board() -> Result<BoardInfo, String> {
    if let Some(f) = std::env::args().nth(1) {
        return load_board_file(std::path::Path::new(&f));
    }
    let client = KiCadClientBlocking::builder()
        .client_name("kipanel")
        .connect()
        .map_err(|e| format!("Connect failed: {e}"))?;
    let ver = client
        .get_version()
        .map(|v| v.full_version)
        .unwrap_or_else(|e| format!("? ({e})"));
    if !client.has_open_board().map_err(|e| format!("{e}"))? {
        return Err(format!("KiCad {ver}: no board open in PCB editor"));
    }
    let text = client
        .get_board_as_string()
        .map_err(|e| format!("get_board_as_string: {e}"))?;
    let root = sexpr::parse(&text)?;
    let outline = board::extract_outline(&root)?;

    // board name + project folder (for config file and export path)
    let docs = client.get_open_documents(kicad_ipc_rs::DocumentType::Pcb).unwrap_or_default();
    let doc = docs.first();
    let stem = doc
        .and_then(|d| d.board_filename.clone())
        .map(|f| {
            std::path::Path::new(&f)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or(f)
        })
        .unwrap_or_else(|| "board".into());
    let project_dir = doc
        .and_then(|d| d.project.path.clone())
        .or_else(|| client.get_current_project_path().ok())
        .map(|p| if p.extension().map(|e| e == "kicad_pro").unwrap_or(false) { p.parent().map(|x| x.to_path_buf()).unwrap_or(p) } else { p });
    let pcbnew = client.get_kicad_binary_path("pcbnew").ok();

    let mut status = format!(
        "KiCad {ver}  ·  {stem}  ·  board {:.2} × {:.2} mm",
        outline.width(),
        outline.height()
    );
    if !outline.open.is_empty() {
        status += &format!("\n⚠ {} open outline segment(s)", outline.open.len());
    }
    if outline.separate > 0 {
        status += &format!("\n⚠ {} separate outline(s) outside the main board ignored", outline.separate);
    }
    Ok(BoardInfo { status, outline, text, stem, project_dir, pcbnew })
}

const LABEL_W: f32 = 150.0;
/// shortcut modifier as shown in tooltips (egui maps COMMAND to Cmd on macOS, Ctrl elsewhere)
const MOD_KEY: &str = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };

/// fixed-width, left-aligned field label
fn field_label(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(LABEL_W, 22.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(LABEL_W);
            ui.add(egui::Label::new(text).truncate())
        },
    )
    .inner
}
const ACCENT: Color32 = Color32::from_rgb(25, 118, 210);
const OK_TEXT: Color32 = Color32::from_rgb(140, 225, 150);
const ERR_TEXT: Color32 = Color32::from_rgb(255, 135, 135);
const WARN_TEXT: Color32 = Color32::from_rgb(240, 205, 110);
const MUTED: Color32 = Color32::from_rgb(150, 158, 172);
/// file paths: bright, so they stand out from the muted labels
const PATH_TEXT: Color32 = Color32::from_rgb(170, 208, 255);

fn setup_style(ctx: &egui::Context) {
    use egui::{FontFamily, FontId, TextStyle};
    // all colours are tuned for a dark UI (like the PCB canvas), so don't follow a light system theme
    ctx.set_theme(egui::Theme::Dark);
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(27, 30, 36);
    v.window_fill = Color32::from_rgb(33, 37, 44);
    v.extreme_bg_color = Color32::from_rgb(15, 17, 21);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    v.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(208, 213, 222);
    v.widgets.noninteractive.bg_stroke.color = Color32::from_rgb(52, 57, 67);
    v.widgets.inactive.fg_stroke.color = Color32::from_rgb(228, 231, 237);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(50, 55, 65);
    v.widgets.inactive.bg_fill = Color32::from_rgb(50, 55, 65);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(66, 73, 87);
    v.widgets.hovered.bg_fill = Color32::from_rgb(66, 73, 87);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(110, 150, 210));
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = egui::CornerRadius::same(5);
    }
    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.5, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(12.5, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.5, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 7.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 24.0;
    });
}

/// big full-width button
fn big_button(ui: &mut egui::Ui, enabled: bool, text: &str, fill: Option<Color32>) -> egui::Response {
    let mut rt = egui::RichText::new(text).size(15.5);
    if fill.is_some() {
        rt = rt.strong().color(Color32::WHITE);
    }
    let mut b = egui::Button::new(rt).min_size(egui::vec2(ui.available_width(), 36.0));
    if let Some(f) = fill {
        b = b.fill(f);
    }
    ui.add_enabled(enabled, b)
}

/// shorten a long path in the middle, keeping the start and (more of) the file name end
fn elide_middle(s: &str, max_chars: usize) -> String {
    let n = s.chars().count();
    if n <= max_chars || max_chars < 8 {
        return s.to_string();
    }
    let head = (max_chars - 1) / 3;
    let tail = max_chars - 1 - head;
    let a: String = s.chars().take(head).collect();
    let b: String = s.chars().skip(n - tail).collect();
    format!("{a}…{b}")
}

/// show a file in the system file manager (or its nearest existing folder)
fn reveal(p: &std::path::Path) {
    #[cfg(windows)]
    if p.exists() {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer").raw_arg(format!("/select,\"{}\"", p.display())).spawn();
        return;
    }
    let mut dir = p.parent();
    while let Some(d) = dir {
        if d.is_dir() {
            break;
        }
        dir = d.parent();
    }
    let Some(dir) = dir else { return };
    let opener = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(opener).arg(dir).spawn();
}

/// a clearly visible path: elided to the available width, full path on hover, click shows it in the folder
fn path_link(ui: &mut egui::Ui, p: &std::path::Path, size: f32) {
    let font = egui::FontId::monospace(size);
    let cw = ui.fonts(|f| f.glyph_width(&font, '0')).max(1.0);
    let full = p.display().to_string();
    let shown = elide_middle(&full, ((ui.available_width() - 14.0) / cw).floor().max(8.0) as usize);
    let r = ui
        .add(egui::Label::new(egui::RichText::new(shown).font(font).color(PATH_TEXT)).sense(Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{full}\n\nClick to show in folder"));
    if r.hovered() {
        let y = r.rect.bottom() - 1.0;
        ui.painter().hline(r.rect.x_range(), y, Stroke::new(1.0_f32, PATH_TEXT));
    }
    if r.clicked() {
        reveal(p);
    }
}

/// "Panel   D:\…\board_panel.kicad_pcb" row in the footer
fn file_row(ui: &mut egui::Ui, label: &str, p: Option<PathBuf>) {
    let Some(p) = p else { return };
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(58.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(58.0);
            ui.label(egui::RichText::new(label).small().color(MUTED));
        });
        path_link(ui, &p, 12.5);
    });
}

/// text field for a path with "…" browse button; returns true if browse clicked
fn path_field(ui: &mut egui::Ui, label: &str, v: &mut String, hint: &str, help: &str) -> bool {
    ui.horizontal(|ui| {
        field_label(ui, label);
        let browse = ui.button("…").on_hover_text("Browse");
        ui.add_sized([ui.available_width() - 6.0, 22.0], egui::TextEdit::singleline(v).hint_text(hint))
            .on_hover_text(help);
        browse.clicked()
    })
    .inner
}

const BG: Color32 = Color32::from_rgb(0, 16, 36);
const SUBSTRATE: Color32 = Color32::from_rgb(28, 82, 52);
const BOARD: Color32 = Color32::from_rgb(38, 110, 66);
const EDGE: Color32 = Color32::from_rgb(225, 225, 205);
const HL: Color32 = Color32::from_rgba_premultiplied(120, 110, 0, 120);
const HL_STROKE: Color32 = Color32::from_rgb(255, 225, 0);
const DIM: Color32 = Color32::from_rgb(160, 180, 220);
const VCUT: Color32 = Color32::from_rgb(255, 150, 40);
const BAD: Color32 = Color32::from_rgb(255, 60, 60);

/// Numeric field in mm with label, explanation on hover. Returns true if hovered.
fn mm_field(ui: &mut egui::Ui, label: &str, v: &mut f64, range: std::ops::RangeInclusive<f64>, help: &str) -> bool {
    ui.horizontal(|ui| {
        let l = field_label(ui, label);
        let d = ui.add(egui::DragValue::new(v).range(range).speed(0.1).suffix(" mm").max_decimals(2));
        let hovered = l.hovered() || d.hovered() || d.has_focus() || d.dragged();
        d.on_hover_text(help);
        hovered
    })
    .inner
}

fn int_field(ui: &mut egui::Ui, label: &str, v: &mut u32, range: std::ops::RangeInclusive<u32>, help: &str) -> bool {
    ui.horizontal(|ui| {
        let l = field_label(ui, label);
        let d = ui.add(egui::DragValue::new(v).range(range).speed(0.05));
        let hovered = l.hovered() || d.hovered() || d.has_focus() || d.dragged();
        d.on_hover_text(help);
        hovered
    })
    .inner
}

fn section_title(t: &str) -> egui::RichText {
    egui::RichText::new(t).strong().size(16.5).color(Color32::from_rgb(140, 190, 255))
}

fn section_help(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().color(Color32::GRAY));
    ui.add_space(4.0);
}

impl App {
    fn new() -> Self {
        let mut app = App {
            status: String::new(),
            outline: None,
            source: None,
            board_stem: "board".into(),
            project_dir: None,
            pcbnew: None,
            notice: Notice::default(),
            saved_json: None,
            last_export: None,
            pending: None,
            cfg: PanelConfig::default(),
            hl: Hl::None,
            warnings: Vec::new(),
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit_pending: true,
        };
        app.reload();
        app
    }

    fn reload(&mut self) {
        match load_board() {
            Ok(b) => {
                self.status = b.status;
                self.outline = Some(b.outline);
                self.source = Some(b.text);
                self.board_stem = b.stem;
                self.project_dir = b.project_dir;
                self.pcbnew = b.pcbnew;
                self.load_config();
                self.saved_json = self.cfg_json();
            }
            Err(e) => {
                self.status = e;
                self.outline = None;
                self.source = None;
            }
        }
        self.fit_pending = true;
    }

    /// empty -> default, relative -> relative to project folder
    fn resolve(&self, s: &str, default: PathBuf) -> PathBuf {
        let s = s.trim();
        if s.is_empty() {
            return default;
        }
        let p = PathBuf::from(s);
        if p.is_absolute() {
            p
        } else {
            self.project_dir.clone().unwrap_or_default().join(p)
        }
    }

    fn default_config_path(&self) -> Option<PathBuf> {
        self.project_dir.as_ref().map(|d| d.join(format!("{}.kipanel.json", self.board_stem)))
    }

    fn config_path(&self) -> Option<PathBuf> {
        let d = self.default_config_path()?;
        Some(self.resolve(&self.cfg.settings_file, d))
    }

    fn out_dir(&self) -> Option<PathBuf> {
        let d = self.project_dir.as_ref()?.join("panel");
        Some(self.resolve(&self.cfg.out_dir, d))
    }

    fn out_file(&self) -> Option<PathBuf> {
        Some(self.out_dir()?.join(format!("{}{}.kicad_pcb", self.board_stem, self.cfg.out_postfix)))
    }

    fn cfg_json(&self) -> Option<String> {
        serde_json::to_string(&self.cfg).ok()
    }

    /// settings differ from what was last loaded / saved
    fn dirty(&self) -> bool {
        self.saved_json.is_some() && self.saved_json != self.cfg_json()
    }

    fn save_settings(&mut self) {
        self.notice = match self.save_config() {
            Ok(p) => Notice::ok_path("✔ Settings saved", p),
            Err(e) => Notice::err(format!("⚠ Settings not saved: {e}")),
        };
    }

    fn load_config_from(&mut self, p: &std::path::Path) -> Result<(), String> {
        let s = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        self.cfg = serde_json::from_str::<PanelConfig>(&s).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(())
    }

    fn load_config(&mut self) {
        if let Some(p) = self.default_config_path() {
            if let Ok(s) = std::fs::read_to_string(&p) {
                if let Ok(c) = serde_json::from_str::<PanelConfig>(&s) {
                    self.cfg = c;
                }
            }
        }
    }

    fn save_config(&mut self) -> Result<PathBuf, String> {
        let p = self.config_path().ok_or("project folder unknown")?;
        let s = serde_json::to_string_pretty(&self.cfg).map_err(|e| e.to_string())?;
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(&p, s).map_err(|e| format!("{}: {e}", p.display()))?;
        // remember a custom location in the default file, so it is found next time
        if let Some(d) = self.default_config_path() {
            if d != p {
                let _ = std::fs::write(&d, serde_json::to_string_pretty(&self.cfg).unwrap_or_default());
            }
        }
        self.saved_json = self.cfg_json();
        Ok(p)
    }

    fn compute(&self) -> Option<(panel::Panel, panel::Sep, panel::Features)> {
        let o = self.outline.as_ref()?;
        let p = panel::build(o, &self.cfg);
        let sep = panel::separation(&p, &self.cfg);
        let feats = panel::features(&p, &self.cfg, &sep.vcuts);
        Some((p, sep, feats))
    }

    fn do_export(&mut self) {
        let res = (|| -> Result<(String, PathBuf), String> {
            let src = self.source.clone().ok_or("no board loaded")?;
            let (p, sep, feats) = self.compute().ok_or("no board loaded")?;
            let (text, st) = export::export(&src, &p, &self.cfg, &sep, &feats)?;
            let out = self.out_file().ok_or("project folder unknown")?;
            let dir = out.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            std::fs::write(&out, text).map_err(|e| format!("{}: {e}", out.display()))?;
            // copy project file so design rules / net classes come along
            if let (true, Some(pd)) = (self.cfg.copy_pro, &self.project_dir) {
                if let Ok(rd) = std::fs::read_dir(pd) {
                    for e in rd.flatten() {
                        let path = e.path();
                        if path.extension().map(|x| x == "kicad_pro").unwrap_or(false) {
                            let _ = std::fs::copy(&path, dir.join(format!("{}{}.kicad_pro", self.board_stem, self.cfg.out_postfix)));
                            break;
                        }
                    }
                }
            }
            let _ = self.save_config();
            self.last_export = Some(out.clone());
            let msg = format!(
                "✔ Exported {} boards ({} items each), {} outline ring(s), {} mousebite holes",
                st.boards, st.items_per_board, st.outline_rings, st.holes
            );
            Ok((msg, out))
        })();
        let ok = res.is_ok();
        self.notice = match res {
            Ok((msg, out)) => Notice::ok_path(msg, out),
            Err(e) => Notice::err(format!("⚠ Export failed: {e}")),
        };
        if ok && self.cfg.open_after {
            self.open_in_kicad();
        }
    }

    fn open_in_kicad(&mut self) {
        let Some(f) = self.last_export.clone() else { return };
        let bin = self.pcbnew.clone().unwrap_or_else(|| {
            // file mode: no path from KiCad; on macOS pcbnew lives inside the app bundle, not on PATH
            let mac = "/Applications/KiCad/KiCad.app/Contents/Applications/pcbnew.app/Contents/MacOS/pcbnew";
            if cfg!(target_os = "macos") && std::path::Path::new(mac).exists() { mac.into() } else { "pcbnew".into() }
        });
        if let Err(e) = std::process::Command::new(&bin).arg(&f).spawn() {
            self.notice = Notice::err(format!("⚠ Could not start {bin}: {e}"));
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let mut pending: Option<Pending> = None;
        let settings_path = self.config_path();
        let c = &mut self.cfg;
        let mut hl = Hl::None;

        egui::CollapsingHeader::new(section_title("Layout"))
            .default_open(true)
            .show(ui, |ui| {
                section_help(ui, "How many copies of the board, and the distance between them. For V-cut separation use 0 mm spacing; for tabs / mousebites typically 2–3 mm.");
                if int_field(ui, "Columns", &mut c.cols, 1..=50, "Number of boards side by side (X)") { hl = Hl::Grid; }
                if int_field(ui, "Rows", &mut c.rows, 1..=50, "Number of boards stacked (Y)") { hl = Hl::Grid; }
                if mm_field(ui, "Column spacing", &mut c.hspace, 0.0..=50.0, "Gap between board edges of neighbouring columns") { hl = Hl::HSpace; }
                if mm_field(ui, "Row spacing", &mut c.vspace, 0.0..=50.0, "Gap between board edges of neighbouring rows") { hl = Hl::VSpace; }
                ui.horizontal(|ui| {
                    field_label(ui, "Board rotation");
                    for deg in [0u32, 90, 180, 270] {
                        ui.selectable_value(&mut c.rotation, deg, format!("{deg}°"));
                    }
                });
            });

        ui.add_space(6.0);
        egui::CollapsingHeader::new(section_title("Frame / rails"))
            .default_open(true)
            .show(ui, |ui| {
                section_help(ui, "Extra material around the boards. Assembly lines need rails (typically ≥ 5 mm) for conveyor transport, tooling holes and fiducials.");
                let r = egui::ComboBox::from_id_salt("framing")
                    .selected_text(c.framing.label())
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for f in Framing::ALL {
                            ui.selectable_value(&mut c.framing, f, f.label());
                        }
                    });
                if r.response.hovered() { hl = Hl::Rails; }
                if c.framing != Framing::None {
                    if mm_field(ui, "Rail width", &mut c.rail_width, 1.0..=30.0, "Width of each rail / frame bar") { hl = Hl::Rails; }
                    if mm_field(ui, "Gap to boards", &mut c.rail_space, 0.0..=20.0, "Distance between rail and board edges (0 for V-cut)") { hl = Hl::RailGap; }
                    if mm_field(ui, "Min panel width", &mut c.min_w, 0.0..=500.0, "Rails grow until the panel is at least this wide (0 = off). Fabs often require ≥ 70 mm for V-cut panels.") { hl = Hl::Rails; }
                    if mm_field(ui, "Min panel height", &mut c.min_h, 0.0..=500.0, "Rails grow until the panel is at least this high (0 = off)") { hl = Hl::Rails; }
                }
            });

        ui.add_space(6.0);
        egui::CollapsingHeader::new(section_title("Separation"))
            .default_open(true)
            .show(ui, |ui| {
                section_help(ui, "How boards are broken out of the panel.\n• V-cut: scored grooves top+bottom, straight lines across the whole panel, needs 0 mm spacing.\n• Mousebites: tabs with a row of small holes, break cleanly; works for any shape.\n• Tabs only: solid tabs, removed with a cutter / depaneling tool.");
                let r = egui::ComboBox::from_id_salt("sep")
                    .selected_text(c.sep.label())
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for s in Separation::ALL {
                            ui.selectable_value(&mut c.sep, s, s.label());
                        }
                    });
                if r.response.hovered() {
                    hl = if c.sep == Separation::VCut { Hl::VCuts } else { Hl::Tabs };
                }
                match c.sep {
                    Separation::VCut => {
                        ui.label(egui::RichText::new("V-cut lines are placed on every board edge that lies on the board's bounding box. Orange = OK, red = crosses a board.").small().color(Color32::GRAY));
                        if ui.button("Set all spacings to 0 mm").clicked() {
                            c.hspace = 0.0;
                            c.vspace = 0.0;
                            c.rail_space = 0.0;
                        }
                    }
                    _ => {
                        if mm_field(ui, "Tab width", &mut c.tab_width, 0.5..=30.0, "Width of each tab along the board edge") { hl = Hl::Tabs; }
                        if int_field(ui, "Tabs top/bottom", &mut c.tabs_h, 0..=10, "Tabs per top and per bottom board edge") { hl = Hl::Tabs; }
                        if int_field(ui, "Tabs left/right", &mut c.tabs_v, 0..=10, "Tabs per left and per right board edge") { hl = Hl::Tabs; }
                        if c.sep == Separation::Mousebites {
                            if mm_field(ui, "Hole Ø", &mut c.mb_drill, 0.2..=2.0, "Mousebite drill diameter (NPTH). 0.5 mm is common.") { hl = Hl::Holes; }
                            if mm_field(ui, "Hole pitch", &mut c.mb_spacing, 0.3..=3.0, "Center-to-center distance of the holes. Web between holes = pitch − Ø.") { hl = Hl::Holes; }
                            if mm_field(ui, "Offset into board", &mut c.mb_offset, -1.0..=1.0, "Hole centers are moved this far into the board, so the broken edge stays inside the outline (no burr).") { hl = Hl::Holes; }
                        }
                        if mm_field(ui, "Router Ø", &mut c.mill_d, 0.5..=3.0, "Fab's milling bit diameter. Gaps between boards must be at least this wide. JLC/PCBWay: ~1.6–2.0 mm.") { hl = Hl::Mill; }
                    }
                }
            });

        ui.add_space(6.0);
        egui::CollapsingHeader::new(section_title("Tooling holes & fiducials"))
            .default_open(true)
            .show(ui, |ui| {
                section_help(ui, "For SMT assembly. Tooling holes locate the panel on the machine, fiducials are copper dots the pick-and-place camera aligns to. 3 = asymmetric pattern (panel can't be loaded rotated). Both sit on the rails.");
                ui.horizontal(|ui| {
                    field_label(ui, "Tooling holes");
                    let r = egui::ComboBox::from_id_salt("tooling").selected_text(c.tooling.label()).show_ui(ui, |ui| {
                        for v in Corners::ALL { ui.selectable_value(&mut c.tooling, v, v.label()); }
                    });
                    if r.response.hovered() { hl = Hl::Tooling; }
                });
                if c.tooling != Corners::None {
                    if mm_field(ui, "Hole Ø", &mut c.tool_d, 0.5..=5.0, "Tooling hole diameter. JLCPCB uses 1.152 mm, many others 1.5–3 mm.") { hl = Hl::Tooling; }
                    if mm_field(ui, "Distance from corner", &mut c.tool_along, 1.0..=100.0, "Hole center distance from the panel corner along the rail. Across the rail the hole is centered.") { hl = Hl::Tooling; }
                    let r = ui.checkbox(&mut c.tool_pth, "Plated (PTH) instead of NPTH");
                    if r.hovered() { hl = Hl::Tooling; }
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    field_label(ui, "Fiducials");
                    let r = egui::ComboBox::from_id_salt("fids").selected_text(c.fiducials.label()).show_ui(ui, |ui| {
                        for v in Corners::ALL { ui.selectable_value(&mut c.fiducials, v, v.label()); }
                    });
                    if r.response.hovered() { hl = Hl::Fiducials; }
                });
                if c.fiducials != Corners::None {
                    if mm_field(ui, "Copper Ø", &mut c.fid_copper, 0.3..=3.0, "Exposed copper dot diameter (typ. 1 mm)") { hl = Hl::Fiducials; }
                    if mm_field(ui, "Mask opening Ø", &mut c.fid_opening, 0.5..=5.0, "Solder mask clearance around the dot (typ. 2× copper)") { hl = Hl::Fiducials; }
                    if mm_field(ui, "Distance from corner", &mut c.fid_along, 1.0..=100.0, "Fiducial center distance from the panel corner along the rail") { hl = Hl::Fiducials; }
                    ui.horizontal(|ui| {
                        field_label(ui, "Side");
                        ui.selectable_value(&mut c.fid_side, FidSide::Top, "Top");
                        ui.selectable_value(&mut c.fid_side, FidSide::Bottom, "Bottom");
                        ui.selectable_value(&mut c.fid_side, FidSide::Both, "Both");
                    });
                }
            });

        ui.add_space(6.0);
        egui::CollapsingHeader::new(section_title("Panel text"))
            .default_open(false)
            .show(ui, |ui| {
                section_help(ui, "Silkscreen text centered on a rail, e.g. version / date, or JLCJLCJLCJLC to mark where JLC prints its order number.");
                let r = ui.horizontal(|ui| {
                    field_label(ui, "Text");
                    ui.text_edit_singleline(&mut c.text)
                });
                if r.inner.hovered() || r.inner.has_focus() { hl = Hl::Text; }
                if mm_field(ui, "Text height", &mut c.text_size, 0.5..=5.0, "Character height on silkscreen") { hl = Hl::Text; }
                ui.horizontal(|ui| {
                    field_label(ui, "Rail");
                    ui.selectable_value(&mut c.text_pos, TextPos::TopRail, "Top / left");
                    ui.selectable_value(&mut c.text_pos, TextPos::BottomRail, "Bottom / right");
                });
            });

        ui.add_space(6.0);
        egui::CollapsingHeader::new(section_title("Output file"))
            .default_open(true)
            .show(ui, |ui| {
                section_help(ui, "Where the panel is written. Empty folder = <project>/panel. Relative paths are relative to the project folder. Each board copy gets its own nets (Board_N-…), Edge.Cuts becomes the panel outline.");
                if path_field(ui, "Folder", &mut c.out_dir, "<project>/panel", "Output folder for the panel files") {
                    pending = Some(Pending::BrowseOutDir);
                }
                ui.horizontal(|ui| {
                    field_label(ui, "File name postfix");
                    ui.add_sized([ui.available_width() - 6.0, 22.0], egui::TextEdit::singleline(&mut c.out_postfix).hint_text("_panel"))
                        .on_hover_text("Appended to the board name: <board><postfix>.kicad_pcb");
                });
                ui.checkbox(&mut c.copy_pro, "Copy project file (.kicad_pro) next to it")
                    .on_hover_text("Keeps design rules and net classes for DRC on the panel.");
                ui.checkbox(&mut c.open_after, "Open in KiCad after export");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    field_label(ui, "V-cut layer");
                    egui::ComboBox::from_id_salt("vlayer").selected_text(c.vcut_layer.clone()).show_ui(ui, |ui| {
                        for l in ["Cmts.User", "Dwgs.User", "User.1", "Edge.Cuts"] {
                            ui.selectable_value(&mut c.vcut_layer, l.to_string(), l);
                        }
                    });
                }).response.on_hover_text("Layer for V-cut lines. Tell your fab which layer it is (JLC: note in the order).");
                ui.checkbox(&mut c.rename_refs, "Rename references per board (R1 becomes R1_2)")
                    .on_hover_text("Unique reference designators — needed by some assembly services when the panel is assembled as one BOM.");
                ui.horizontal(|ui| {
                    field_label(ui, "Panel position");
                    ui.add(egui::DragValue::new(&mut c.origin.0).speed(1.0).prefix("x ").suffix(" mm"));
                    ui.add(egui::DragValue::new(&mut c.origin.1).speed(1.0).prefix("y ").suffix(" mm"));
                }).response.on_hover_text("Top-left corner of the panel on the KiCad page");
            });

        ui.add_space(6.0);
        egui::CollapsingHeader::new(section_title("Settings file"))
            .default_open(false)
            .show(ui, |ui| {
                section_help(ui, "Panel settings are stored as JSON and loaded automatically when you open this board again. Empty = <project>/<board>.kipanel.json.");
                if path_field(ui, "File", &mut c.settings_file, "<project>/<board>.kipanel.json", "Where 'Save settings' writes") {
                    pending = Some(Pending::BrowseSettings);
                }
                if let Some(p) = &settings_path {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Saves to").small().color(MUTED));
                        path_link(ui, p, 12.5);
                    });
                }
            });

        self.hl = hl;
        if pending.is_some() {
            self.pending = pending;
        }
    }

    fn run_pending(&mut self) {
        let Some(p) = self.pending.take() else { return };
        let start = self.project_dir.clone().unwrap_or_default();
        match p {
            Pending::BrowseOutDir => {
                if let Some(d) = rfd::FileDialog::new().set_directory(&start).pick_folder() {
                    self.cfg.out_dir = self.relativize(&d);
                }
            }
            Pending::BrowseSettings => {
                if let Some(f) = rfd::FileDialog::new()
                    .set_directory(&start)
                    .set_file_name(format!("{}.kipanel.json", self.board_stem))
                    .add_filter("KiPanel settings", &["json"])
                    .save_file()
                {
                    self.cfg.settings_file = self.relativize(&f);
                }
            }
        }
    }

    /// store paths inside the project folder as relative paths
    fn relativize(&self, p: &std::path::Path) -> String {
        if let Some(pd) = &self.project_dir {
            if let Ok(r) = p.strip_prefix(pd) {
                return r.display().to_string();
            }
        }
        p.display().to_string()
    }

    fn load_settings_dialog(&mut self) {
        let start = self.project_dir.clone().unwrap_or_default();
        if let Some(f) = rfd::FileDialog::new().set_directory(&start).add_filter("KiPanel settings", &["json"]).pick_file() {
            self.notice = match self.load_config_from(&f) {
                Ok(()) => Notice::ok_path("✔ Settings loaded", f),
                Err(e) => Notice::err(format!("⚠ {e}")),
            };
            self.fit_pending = true;
        }
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, BG);

        let Some(o) = &self.outline else {
            painter.text(rect.center() - Vec2::new(0.0, 14.0), egui::Align2::CENTER_CENTER, "No board loaded", egui::FontId::proportional(22.0), Color32::from_rgb(210, 220, 240));
            painter.text(
                rect.center() + Vec2::new(0.0, 16.0),
                egui::Align2::CENTER_CENTER,
                "Open a board in the KiCad PCB editor (API server enabled), then press “Reload board”.",
                egui::FontId::proportional(14.5),
                DIM,
            );
            return;
        };
        let p = panel::build(o, &self.cfg);
        let sep = panel::separation(&p, &self.cfg);
        let feats = panel::features(&p, &self.cfg, &sep.vcuts);
        self.warnings = sep.warnings.clone();
        self.warnings.extend(feats.warnings.iter().cloned());
        let (pw, ph) = p.size;

        if self.fit_pending {
            self.zoom = (rect.width() / pw as f32).min(rect.height() / ph as f32) * 0.85;
            self.pan = Vec2::ZERO;
            self.fit_pending = false;
        }
        if resp.dragged() {
            self.pan += resp.drag_delta();
        }
        if let Some(hover) = resp.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let factor = (scroll * 0.002).exp();
                let anchor = hover - rect.center() - self.pan;
                self.pan -= anchor * (factor - 1.0);
                self.zoom *= factor;
            }
        }
        if resp.double_clicked() {
            self.fit_pending = true;
        }

        let zoom = self.zoom;
        let origin = rect.center() + self.pan;
        let center = (pw / 2.0, ph / 2.0);
        let ts = |q: (f64, f64)| -> Pos2 {
            origin + Vec2::new(((q.0 - center.0) as f32) * zoom, ((q.1 - center.1) as f32) * zoom)
        };
        let srect = |r: &RectMm| Rect::from_two_pos(ts(r.min), ts(r.max));

        // rails
        for r in &p.rails {
            painter.rect_filled(srect(r), 0.0, SUBSTRATE);
            painter.rect_stroke(srect(r), 0.0, Stroke::new(1.0_f32, EDGE), egui::StrokeKind::Middle);
        }

        // extra substrate: tabs + filled gaps
        for r in sep.tabs.iter().chain(sep.fills.iter()) {
            painter.rect_filled(srect(r), 0.0, SUBSTRATE);
        }

        // boards: fill (ear clipping) + outline
        for b in &p.boards {
            for (li, l) in b.loops.iter().enumerate() {
                let pts: Vec<Pos2> = l.iter().map(|&q| ts(q)).collect();
                let fill = if li == 0 { BOARD } else { BG };
                let mut mesh = egui::Mesh::default();
                for &q in &pts {
                    mesh.colored_vertex(q, fill);
                }
                for t in panel::triangulate(l) {
                    mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
                }
                painter.add(egui::Shape::mesh(mesh));
                painter.add(egui::Shape::closed_line(pts, Stroke::new(1.2_f32, EDGE)));
            }
        }

        // mousebite holes
        for &(c, d) in &sep.holes {
            painter.circle(ts(c), (d as f32 / 2.0 * zoom).max(1.0), BG, Stroke::new(0.8_f32, EDGE));
        }
        // break lines (tabs only)
        for &(a, b) in &sep.breaks {
            painter.add(egui::Shape::dashed_line(&[ts(a), ts(b)], Stroke::new(1.5_f32, Color32::WHITE), 4.0, 3.0));
        }
        // V-cut lines across the whole panel
        let vw = if self.hl == Hl::VCuts { 2.5_f32 } else { 1.5_f32 };
        for v in &sep.vcuts {
            let (a, b) = if v.vertical { ((v.pos, -2.0), (v.pos, ph + 2.0)) } else { ((-2.0, v.pos), (pw + 2.0, v.pos)) };
            let col = if v.conflict { BAD } else { VCUT };
            painter.add(egui::Shape::dashed_line(&[ts(a), ts(b)], Stroke::new(vw, col), 8.0, 4.0));
        }

        // tooling holes / fiducials
        for f in &feats.items {
            let c = ts(f.pos);
            match f.kind {
                FeatureKind::Tooling => {
                    painter.circle(c, (self.cfg.tool_d as f32 / 2.0 * zoom).max(2.0), BG, Stroke::new(1.0_f32, EDGE));
                }
                FeatureKind::FiducialTop | FeatureKind::FiducialBottom => {
                    let col = if f.kind == FeatureKind::FiducialTop { Color32::from_rgb(230, 190, 80) } else { Color32::from_rgb(120, 170, 255) };
                    painter.circle_filled(c, (self.cfg.fid_opening as f32 / 2.0 * zoom).max(2.0), Color32::from_rgb(15, 45, 30));
                    painter.circle_filled(c, (self.cfg.fid_copper as f32 / 2.0 * zoom).max(1.0), col);
                }
            }
        }
        if let Some(t) = &feats.text {
            painter.text(
                ts(t.pos),
                egui::Align2::CENTER_CENTER,
                &t.text,
                egui::FontId::monospace((t.size as f32 * zoom * 1.1).max(4.0)),
                Color32::from_rgb(235, 235, 235),
            );
        }

        // highlight
        let hole_boxes = || -> Vec<RectMm> {
            sep.holes.iter().map(|&(c, d)| RectMm::new(c.0 - d / 2.0, c.1 - d / 2.0, c.0 + d / 2.0, c.1 + d / 2.0)).collect()
        };
        let hl_rects: Vec<RectMm> = match self.hl {
            Hl::Tabs => sep.tabs.clone(),
            Hl::Holes => hole_boxes(),
            Hl::Mill => p.hgaps.iter().chain(p.vgaps.iter()).chain(p.rail_gaps.iter()).copied().collect(),
            Hl::VCuts => vec![],
            Hl::Tooling => feats.items.iter().filter(|f| f.kind == FeatureKind::Tooling).map(|f| RectMm::new(f.pos.0 - f.r, f.pos.1 - f.r, f.pos.0 + f.r, f.pos.1 + f.r)).collect(),
            Hl::Fiducials => feats.items.iter().filter(|f| f.kind != FeatureKind::Tooling).map(|f| RectMm::new(f.pos.0 - f.r, f.pos.1 - f.r, f.pos.0 + f.r, f.pos.1 + f.r)).collect(),
            Hl::Text => feats.text.iter().map(|t| t.rect).collect(),
            Hl::HSpace => p.hgaps.clone(),
            Hl::VSpace => p.vgaps.clone(),
            Hl::Rails => p.rails.clone(),
            Hl::RailGap => p.rail_gaps.clone(),
            Hl::Grid => p.boards.iter().map(|b| b.bbox).collect(),
            Hl::None => vec![],
        };
        for r in &hl_rects {
            let sr = srect(r);
            // make zero-width gaps visible
            let sr = if sr.width() < 2.0 { sr.expand2(Vec2::new(2.0, 0.0)) } else { sr };
            let sr = if sr.height() < 2.0 { sr.expand2(Vec2::new(0.0, 2.0)) } else { sr };
            painter.rect_filled(sr, 0.0, HL);
            painter.rect_stroke(sr, 0.0, Stroke::new(1.0_f32, HL_STROKE), egui::StrokeKind::Middle);
        }

        // panel dimensions
        let full = srect(&RectMm::new(0.0, 0.0, pw, ph));
        painter.rect_stroke(full, 0.0, Stroke::new(0.5_f32, Color32::from_rgb(80, 110, 160)), egui::StrokeKind::Outside);
        painter.text(
            full.center_bottom() + Vec2::new(0.0, 16.0),
            egui::Align2::CENTER_CENTER,
            format!("Panel {:.2} × {:.2} mm  ·  {} boards", pw, ph, p.boards.len()),
            egui::FontId::proportional(15.0),
            DIM,
        );
        painter.text(
            rect.left_bottom() + Vec2::new(10.0, -8.0),
            egui::Align2::LEFT_BOTTOM,
            "drag = pan  ·  wheel = zoom  ·  double-click = fit",
            egui::FontId::proportional(12.5),
            Color32::from_rgb(110, 130, 170),
        );
        let _ = p.area;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ---------- shortcuts
        if self.source.is_some() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
                self.save_settings();
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::E)) {
                self.do_export();
            }
        }

        // ---------- top toolbar
        egui::TopBottomPanel::top("toolbar").frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(10, 8))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("KiPanel");
                ui.add_space(12.0);
                if ui.add(egui::Button::new("⟳  Reload board").min_size(egui::vec2(0.0, 30.0))).on_hover_text("Read the board again from KiCad (includes unsaved changes)").clicked() {
                    self.reload();
                }
                ui.add_space(4.0);
                if ui.add(egui::Button::new("Fit view").min_size(egui::vec2(0.0, 30.0))).clicked() {
                    self.fit_pending = true;
                }
                ui.add_space(4.0);
                let mut chosen: Option<Preset> = None;
                egui::ComboBox::from_id_salt("preset")
                    .selected_text("★  Start from preset…")
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for p in Preset::ALL {
                            if ui.selectable_label(false, p.label()).clicked() {
                                chosen = Some(p);
                            }
                        }
                    })
                    .response
                    .on_hover_text("Fill all settings with a proven combination. Rows, columns, rotation and file settings are kept.");
                if let Some(p) = chosen {
                    p.apply(&mut self.cfg);
                    self.notice = Notice::ok(format!("✔ Preset applied: {}", p.label()));
                    self.fit_pending = true;
                }
                ui.add_space(12.0);
                ui.label(egui::RichText::new(&self.status).color(Color32::from_rgb(190, 200, 215)));
            });
        });

        // ---------- bottom status bar
        egui::TopBottomPanel::bottom("statusbar").frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(10, 6))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let n = &self.notice;
                if n.text.is_empty() {
                    ui.label(egui::RichText::new("Ready").color(MUTED));
                } else {
                    let (fg, bg) = if n.error { (ERR_TEXT, Color32::from_rgb(70, 26, 26)) } else { (OK_TEXT, Color32::from_rgb(22, 58, 34)) };
                    egui::Frame::NONE.fill(bg).corner_radius(4.0).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
                        ui.label(egui::RichText::new(&n.text).strong().color(fg));
                    });
                    if let Some(p) = &n.path {
                        if ui.small_button("Show in folder").clicked() {
                            reveal(p);
                        }
                        path_link(ui, p, 13.5);
                    }
                }
            });
        });

        // ---------- right side: settings (scroll) + fixed footer (checks + actions)
        egui::SidePanel::right("settings")
            .resizable(true)
            .min_width(340.0)
            .default_width(400.0)
            .show(ctx, |ui| {
                // fixed footer: checks + actions
                egui::TopBottomPanel::bottom("actions")
                    .frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 0, right: 0, top: 6, bottom: 8 }))
                    .show_inside(ui, |ui| {
                        let has_board = self.source.is_some();
                        let errors = self.warnings.iter().filter(|w| w.starts_with('⚠')).count();
                        let notes = self.warnings.len() - errors;

                        // checks
                        if !self.warnings.is_empty() {
                            let title = if errors > 0 {
                                egui::RichText::new(format!("⚠ Checks — {errors} problem(s), {notes} note(s)")).color(ERR_TEXT)
                            } else {
                                egui::RichText::new(format!("Checks — {notes} note(s)")).color(WARN_TEXT)
                            };
                            ui.label(title.strong().size(15.5));
                            egui::ScrollArea::vertical().id_salt("checks").max_height(170.0).auto_shrink([false, true]).show(ui, |ui| {
                                for w in &self.warnings {
                                    let err = w.starts_with('⚠');
                                    let (fg, bg) = if err {
                                        (ERR_TEXT, Color32::from_rgb(60, 22, 22))
                                    } else {
                                        (WARN_TEXT, Color32::from_rgb(50, 44, 20))
                                    };
                                    egui::Frame::NONE.fill(bg).corner_radius(4.0).inner_margin(8.0).show(ui, |ui| {
                                        ui.set_width(ui.available_width() - 4.0);
                                        ui.label(egui::RichText::new(w.trim_start_matches("⚠ ")).color(fg));
                                    });
                                    ui.add_space(4.0);
                                }
                            });
                        } else if has_board {
                            ui.label(egui::RichText::new("✔ Checks — no problems").strong().size(15.5).color(OK_TEXT));
                        }
                        ui.add_space(8.0);

                        // export
                        file_row(ui, "Panel", self.out_file());
                        file_row(ui, "Settings", self.config_path());
                        ui.add_space(2.0);
                        let label = if errors > 0 {
                            format!("⬇  Export panel   ({errors} problem{})", if errors == 1 { "" } else { "s" })
                        } else {
                            "⬇  Export panel".to_string()
                        };
                        let fill = if errors > 0 { Color32::from_rgb(170, 90, 20) } else { ACCENT };
                        if big_button(ui, has_board, &label, Some(fill)).on_hover_text(format!("Write the panel file  ·  {MOD_KEY}+E")).clicked() {
                            self.do_export();
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let w = (ui.available_width() - 2.0 * 8.0) / 3.0;
                            let b = |t: &str| egui::Button::new(t).min_size(egui::vec2(w, 32.0));
                            let dirty = self.dirty();
                            let save = if dirty {
                                egui::Button::new(egui::RichText::new("💾  Save settings *").color(WARN_TEXT))
                                    .stroke(Stroke::new(1.0_f32, WARN_TEXT))
                                    .min_size(egui::vec2(w, 32.0))
                            } else {
                                b("💾  Save settings")
                            };
                            let tip = format!(
                                "{}{MOD_KEY}+S\n{}",
                                if dirty { "Unsaved changes  ·  " } else { "" },
                                self.config_path().map(|p| p.display().to_string()).unwrap_or_default()
                            );
                            if ui.add_enabled(has_board, save).on_hover_text(tip).clicked() {
                                self.save_settings();
                            }
                            if ui.add_enabled(has_board, b("📂  Load settings…")).clicked() {
                                self.load_settings_dialog();
                            }
                            if ui
                                .add_enabled(self.last_export.is_some(), b("Open in KiCad"))
                                .on_hover_text("Open the last exported panel in the PCB editor")
                                .clicked()
                            {
                                self.open_in_kicad();
                            }
                        });
                    });

                // settings fill the rest
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show_inside(ui, |ui| {
                        egui::ScrollArea::vertical().id_salt("settings").auto_shrink([false, false]).show(ui, |ui| {
                            ui.add_space(4.0);
                            self.settings(ui);
                            ui.add_space(12.0);
                        });
                    });
            });
        self.run_pending();

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| self.preview(ui));
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 560.0])
            .with_title("KiPanel"),
        ..Default::default()
    };
    eframe::run_native(
        "KiPanel",
        options,
        Box::new(|cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_rect_with_arc() {
        let src = r#"(kicad_pcb (version 20241229)
          (gr_line (start 0 0) (end 10 0) (layer "Edge.Cuts"))
          (gr_line (start 10 0) (end 10 20) (layer "Edge.Cuts"))
          (gr_arc (start 10 20) (mid 5 25) (end 0 20) (layer "Edge.Cuts"))
          (gr_line (start 0 20) (end 0 0) (layer "Edge.Cuts"))
          (gr_circle (center 5 5) (end 6 5) (layer "Edge.Cuts"))
          (gr_line (start 0 0) (end 99 99) (layer "F.SilkS")))"#;
        let root = sexpr::parse(src).unwrap();
        let o = board::extract_outline(&root).unwrap();
        assert_eq!(o.loops.len(), 2);
        assert!(o.open.is_empty());
        assert!((o.width() - 10.0).abs() < 1e-6);
        assert!((o.height() - 25.0).abs() < 1e-3);
    }
}
