#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod board;
mod export;
mod optimize;
mod panel;
mod sexpr;
mod tools;
mod view3d;
mod gl3d;

use board::Outline;
use panel::{Alternation, CornerStyle, Corners, FeatureKind, FidSide, Framing, Justify, OriginCorner, PanelConfig, Preset, RectMm, Separation, TabMode, TextCfg, TextRail};
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
    Bones,
    Copper,
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
    /// final panel outline for the preview, recomputed only when the settings change: (settings, rings)
    outline_cache: Option<(String, std::rc::Rc<export::OutlineGeo>)>,
    /// preview mode: flat, or rotatable 3D view of the bare panel
    view3d: bool,
    cam: view3d::Camera,
    /// board thickness from the board file (mm)
    thickness: f64,
    /// 3D view: draws the board model with its components at every board position
    gl3d: gl3d::Renderer,
    model: ModelLoad,
    /// running kicad-cli job (DRC / render / 3D export)
    job: Option<(tools::Job, std::sync::mpsc::Receiver<Result<tools::Done, String>>)>,
    drc: Option<tools::DrcReport>,
    show_drc: bool,
    render: Option<(PathBuf, egui::TextureHandle)>,
    show_render: bool,
    save_render: bool,
    /// result of the last "Fix automatically": what was changed, and the settings before (for Undo)
    fix_log: Option<(Vec<String>, PanelConfig)>,
}

/// the board model with components for the 3D view (exported by kicad-cli in the background)
enum ModelLoad {
    Idle,
    Loading(std::sync::mpsc::Receiver<Result<gl3d::MeshData, String>>),
    Ready,
    Failed(String),
}

/// above this many boards the exact outline is not drawn in the preview (too slow while dragging)
const OUTLINE_PREVIEW_MAX: usize = 400;

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

/// board thickness from `(general (thickness 1.6))`
fn board_thickness(root: &sexpr::Sexp) -> f64 {
    root.child("general").and_then(|g| g.child("thickness")).and_then(|t| t.f64_at(1)).filter(|t| *t > 0.0).unwrap_or(1.6)
}

struct BoardInfo {
    thickness: f64,
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
    Ok(BoardInfo { thickness: board_thickness(&root), status, outline, text, stem, project_dir: path.parent().map(|p| p.to_path_buf()), pcbnew: None })
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
    Ok(BoardInfo { thickness: board_thickness(&root), status, outline, text, stem, project_dir, pcbnew })
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
const COPPER: Color32 = Color32::from_rgb(176, 120, 58);

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

/// labelled single-line text field
fn text_field(ui: &mut egui::Ui, label: &str, v: &mut String, hint: &str, help: &str) {
    ui.horizontal(|ui| {
        field_label(ui, label);
        ui.add_sized([ui.available_width() - 6.0, 22.0], egui::TextEdit::singleline(v).hint_text(hint)).on_hover_text(help);
    });
}

/// collapsed group for rarely needed options, so the main settings stay short
fn advanced(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(egui::RichText::new("Advanced").color(MUTED)).id_salt(id).default_open(false).show(ui, add);
}

fn load_texture(ctx: &egui::Context, path: &std::path::Path) -> Result<egui::TextureHandle, String> {
    let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Ok(ctx.load_texture("kipanel-render", ci, egui::TextureOptions::LINEAR))
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
            outline_cache: None,
            view3d: false,
            cam: view3d::Camera::default(),
            thickness: 1.6,
            gl3d: gl3d::Renderer::default(),
            model: ModelLoad::Idle,
            job: None,
            drc: None,
            show_drc: false,
            render: None,
            show_render: false,
            save_render: false,
            fix_log: None,
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
                self.thickness = b.thickness;
                self.load_config();
                self.saved_json = self.cfg_json();
            }
            Err(e) => {
                self.status = e;
                self.outline = None;
                self.source = None;
            }
        }
        self.outline_cache = None;
        self.fit_pending = true;
        // dropping the receiver also discards a model export that is still running
        self.model = ModelLoad::Idle;
        self.gl3d.clear_model();
    }

    /// export the board with its components as a 3D model and load it (background thread)
    fn start_model(&mut self, ctx: &egui::Context) {
        let Some(src) = self.source.clone() else { return };
        let cli = tools::find_cli(self.pcbnew.as_deref());
        let (dir, stem) = (self.project_dir.clone(), self.board_stem.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let res = tools::board_glb(&cli, &src, dir.as_deref(), &stem).and_then(|b| gl3d::load_glb(&b));
            let _ = tx.send(res);
            ctx.request_repaint();
        });
        self.model = ModelLoad::Loading(rx);
    }

    fn poll_model(&mut self) {
        let ModelLoad::Loading(rx) = &self.model else { return };
        self.model = match rx.try_recv() {
            Ok(Ok(m)) => {
                self.gl3d.set_model(m);
                ModelLoad::Ready
            }
            Ok(Err(e)) => ModelLoad::Failed(e),
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => ModelLoad::Failed("the model export stopped unexpectedly".into()),
        };
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
        self.cfg = PanelConfig::from_json(&s).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(())
    }

    fn load_config(&mut self) {
        if let Some(p) = self.default_config_path() {
            if let Ok(s) = std::fs::read_to_string(&p) {
                if let Ok(c) = PanelConfig::from_json(&s) {
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
        let feats = panel::features(&p, &self.cfg, &sep.vcuts, &self.board_stem);
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

    /// adjust the settings so the problems in the Checks list go away
    fn auto_fix(&mut self) {
        let Some(o) = &self.outline else { return };
        let (fixed, changes) = optimize::optimize(o, &self.cfg, &self.board_stem);
        if changes.is_empty() {
            self.notice = Notice::err("Nothing here can be fixed automatically — see the Checks list for what to change by hand.");
            return;
        }
        let before = std::mem::replace(&mut self.cfg, fixed);
        self.notice = Notice::ok(format!("✔ {} setting(s) adjusted", changes.len()));
        self.fix_log = Some((changes, before));
        self.fit_pending = true;
    }

    /// the exported panel file the kicad-cli tools work on
    fn panel_file(&self) -> Option<PathBuf> {
        self.last_export.clone().or_else(|| self.out_file()).filter(|f| f.is_file())
    }

    fn start_job(&mut self, ctx: &egui::Context, job: tools::Job) {
        if self.job.is_some() {
            return;
        }
        let Some(panel) = self.panel_file() else {
            self.notice = Notice::err("⚠ Export the panel first");
            return;
        };
        let cli = tools::find_cli(self.pcbnew.as_deref());
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(tools::run(&cli, &panel, job));
            ctx.request_repaint();
        });
        self.notice = Notice::ok(job.label());
        self.job = Some((job, rx));
    }

    fn poll_job(&mut self, ctx: &egui::Context) {
        let Some((_, rx)) = &self.job else { return };
        let res = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the background job stopped unexpectedly".into()),
        };
        self.job = None;
        match res {
            Ok(tools::Done::Drc(r)) => {
                let bad = r.errors + r.unconnected;
                let text = if bad == 0 {
                    format!("✔ DRC: no errors, {} warning(s)", r.warnings)
                } else {
                    format!("⚠ DRC: {} error(s), {} unconnected, {} warning(s)", r.errors, r.unconnected, r.warnings)
                };
                self.notice = Notice { text, path: Some(r.report.clone()), error: bad > 0 };
                self.drc = Some(r);
                self.show_drc = true;
            }
            Ok(tools::Done::Render(p)) => match load_texture(ctx, &p) {
                Ok(t) => {
                    self.notice = Notice::ok_path("✔ 3D image saved", p.clone());
                    self.render = Some((p, t));
                    self.show_render = true;
                }
                Err(e) => self.notice = Notice::err(format!("⚠ Image written, but it could not be shown: {e}")),
            },
            Ok(tools::Done::Model(p)) => self.notice = Notice::ok_path("✔ 3D model saved", p),
            Err(e) => self.notice = Notice::err(format!("⚠ {e}")),
        }
    }

    /// result list of the design rule check
    fn drc_window(&mut self, ctx: &egui::Context) {
        let Some(r) = &self.drc else { return };
        let mut open = self.show_drc;
        egui::Window::new("Design rule check of the panel").open(&mut open).default_size([460.0, 420.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let part = |ui: &mut egui::Ui, n: usize, what: &str, col: Color32| {
                    ui.label(egui::RichText::new(format!("{n} {what}")).strong().color(if n > 0 { col } else { OK_TEXT }));
                };
                part(ui, r.errors, "errors", ERR_TEXT);
                part(ui, r.unconnected, "unconnected", ERR_TEXT);
                part(ui, r.warnings, "warnings", WARN_TEXT);
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Report").small().color(MUTED));
                path_link(ui, &r.report, 12.5);
            });
            if r.items.is_empty() {
                ui.label(egui::RichText::new("✔ KiCad found no problems in the panel.").color(OK_TEXT));
                return;
            }
            ui.label(egui::RichText::new("Marks in the preview show where. Most findings come from the board itself: fix them there, reload and export again.").small().color(Color32::GRAY));
            ui.separator();
            // group by rule, in order of first appearance
            let mut kinds: Vec<&str> = Vec::new();
            for it in &r.items {
                if !kinds.contains(&it.kind.as_str()) {
                    kinds.push(&it.kind);
                }
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for k in kinds {
                    let items: Vec<&tools::DrcItem> = r.items.iter().filter(|i| i.kind == k).collect();
                    let col = if items.iter().any(|i| i.error) { ERR_TEXT } else { WARN_TEXT };
                    let title = format!("{} × {}", items.len(), k.replace('_', " "));
                    egui::CollapsingHeader::new(egui::RichText::new(title).color(col)).id_salt(("drc", k)).show(ui, |ui| {
                        for it in items.iter().take(200) {
                            let mut line = it.text.clone();
                            if let Some((x, y)) = it.pos {
                                line += &format!("  @ {x:.2}, {y:.2} mm");
                            }
                            ui.label(line);
                            for w in &it.what {
                                ui.label(egui::RichText::new(format!("    {w}")).small().color(MUTED));
                            }
                        }
                        if items.len() > 200 {
                            ui.label(egui::RichText::new(format!("… and {} more, see the report file", items.len() - 200)).small().color(MUTED));
                        }
                    });
                }
            });
        });
        self.show_drc = open;
    }

    /// the last 3D picture
    fn render_window(&mut self, ctx: &egui::Context) {
        let Some((path, tex)) = &self.render else { return };
        let mut open = self.show_render;
        let mut save = false;
        egui::Window::new("3D picture of the panel").open(&mut open).default_size([760.0, 540.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Save a copy as…").clicked() {
                    save = true;
                }
                if ui.button("Show in folder").clicked() {
                    reveal(path);
                }
                path_link(ui, path, 12.5);
            });
            let size = tex.size_vec2();
            let avail = ui.available_size();
            let scale = (avail.x / size.x).min(avail.y.max(120.0) / size.y).min(1.0);
            let (rect, _) = ui.allocate_exact_size(size * scale, Sense::hover());
            // neutral grey behind the transparent image: boards are often dark
            ui.painter().rect_filled(rect, 4.0, Color32::from_rgb(96, 102, 114));
            ui.painter().image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        });
        self.show_render = open;
        self.save_render |= save;
    }

    fn save_render_copy(&mut self) {
        if !std::mem::take(&mut self.save_render) {
            return;
        }
        let Some((path, _)) = &self.render else { return };
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "panel.png".into());
        let mut dlg = rfd::FileDialog::new().set_file_name(name).add_filter("PNG image", &["png"]);
        if let Some(dir) = path.parent() {
            dlg = dlg.set_directory(dir);
        }
        if let Some(to) = dlg.save_file() {
            self.notice = match std::fs::copy(path, &to) {
                Ok(_) => Notice::ok_path("✔ Image saved", to),
                Err(e) => Notice::err(format!("⚠ {}: {e}", to.display())),
            };
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
                advanced(ui, "adv_layout", |ui| {
                    ui.horizontal(|ui| {
                        field_label(ui, "Flip boards 180°");
                        let r = egui::ComboBox::from_id_salt("alternation").selected_text(c.alternation.label()).show_ui(ui, |ui| {
                            for a in Alternation::ALL { ui.selectable_value(&mut c.alternation, a, a.label()); }
                        });
                        if r.response.hovered() { hl = Hl::Grid; }
                    })
                    .response
                    .on_hover_text("Turn every other board upside down, e.g. so connectors or odd-shaped edges face each other.");
                    if mm_field(ui, "Extra: 2nd column gap", &mut c.hspace_alt, -50.0..=50.0, "Added to every second gap between columns (may be negative). Useful with flipped boards, where the gaps alternate.") { hl = Hl::HSpace; }
                    if mm_field(ui, "Extra: 2nd row gap", &mut c.vspace_alt, -50.0..=50.0, "Added to every second gap between rows (may be negative)") { hl = Hl::VSpace; }
                    ui.add_space(4.0);
                    if mm_field(ui, "Backbone in col. gaps", &mut c.bone_cols, 0.0..=20.0, "A solid bar in the middle of the gaps between columns; boards are tabbed to it instead of to each other. Makes large panels stiffer. 0 = none. The spacing must be wider than the bar.") { hl = Hl::Bones; }
                    if mm_field(ui, "Backbone in row gaps", &mut c.bone_rows, 0.0..=20.0, "A solid bar in the middle of the gaps between rows. 0 = none.") { hl = Hl::Bones; }
                    if c.bone_cols > 0.0 || c.bone_rows > 0.0 {
                        if int_field(ui, "Backbone every N gaps", &mut c.bone_every, 1..=20, "1 = a backbone in every gap, 2 = in every second gap, …") { hl = Hl::Bones; }
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
                    advanced(ui, "adv_frame", |ui| {
                        if mm_field(ui, "Max panel width", &mut c.max_w, 0.0..=1000.0, "Checks shows a problem if the panel gets wider than this (0 = off). Use your fab's or assembler's limit.") { hl = Hl::Rails; }
                        if mm_field(ui, "Max panel height", &mut c.max_h, 0.0..=1000.0, "Checks shows a problem if the panel gets higher than this (0 = off)") { hl = Hl::Rails; }
                        ui.horizontal(|ui| {
                            field_label(ui, "Panel corners");
                            ui.selectable_value(&mut c.corner, CornerStyle::Square, "Square");
                            ui.selectable_value(&mut c.corner, CornerStyle::Chamfer, "Chamfer");
                            ui.selectable_value(&mut c.corner, CornerStyle::Fillet, "Round");
                        })
                        .response
                        .on_hover_text("Shape of the four outer panel corners. Rounded or chamfered corners do not catch in conveyors and packaging.");
                        if c.corner != CornerStyle::Square {
                            if mm_field(ui, "Corner size", &mut c.corner_size, 0.2..=20.0, "Chamfer length / corner radius") { hl = Hl::Rails; }
                        }
                        let r = ui.checkbox(&mut c.copper_fill, "Fill rails with copper").on_hover_text("Solid copper on both sides of the rails. Evens out plating and makes thin panels stiffer. Kept clear of holes, fiducials and V-cuts.");
                        if r.hovered() { hl = Hl::Copper; }
                        if c.copper_fill {
                            if mm_field(ui, "Copper clearance", &mut c.copper_clearance, 0.2..=5.0, "Distance of the copper from the panel edge, holes, fiducials and V-cut lines") { hl = Hl::Copper; }
                        }
                    });
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
                            c.hspace_alt = 0.0;
                            c.vspace_alt = 0.0;
                            c.rail_space = 0.0;
                        }
                        advanced(ui, "adv_vcut", |ui| {
                            if mm_field(ui, "Extend past edge", &mut c.vcut_extend, 0.0..=20.0, "V-cut lines are drawn this far beyond the panel edge, so they are easy to see in the fab drawing") { hl = Hl::VCuts; }
                        });
                    }
                    _ => {
                        // the left/right width follows the main width until it is set separately
                        let old = c.tab_width;
                        if mm_field(ui, "Tab width", &mut c.tab_width, 0.5..=30.0, "Width of each tab along the board edge") { hl = Hl::Tabs; }
                        if (c.tab_width_lr - old).abs() < 1e-9 {
                            c.tab_width_lr = c.tab_width;
                        }
                        if c.tab_mode == TabMode::Count {
                            if int_field(ui, "Tabs top/bottom", &mut c.tabs_h, 0..=10, "Tabs per top and per bottom board edge") { hl = Hl::Tabs; }
                            if int_field(ui, "Tabs left/right", &mut c.tabs_v, 0..=10, "Tabs per left and per right board edge") { hl = Hl::Tabs; }
                        } else if mm_field(ui, "Tab every", &mut c.tab_spacing, 2.0..=300.0, "Distance between tabs. The number of tabs per edge follows from the edge length.") {
                            hl = Hl::Tabs;
                        }
                        if c.sep == Separation::Mousebites {
                            if mm_field(ui, "Hole Ø", &mut c.mb_drill, 0.2..=2.0, "Mousebite drill diameter (NPTH). 0.5 mm is common.") { hl = Hl::Holes; }
                            if mm_field(ui, "Hole pitch", &mut c.mb_spacing, 0.3..=3.0, "Center-to-center distance of the holes. Web between holes = pitch − Ø.") { hl = Hl::Holes; }
                            if mm_field(ui, "Offset into board", &mut c.mb_offset, -1.0..=1.0, "Hole centers are moved this far into the board, so the broken edge stays inside the outline (no burr).") { hl = Hl::Holes; }
                        }
                        if mm_field(ui, "Router Ø", &mut c.mill_d, 0.5..=3.0, "Fab's milling bit diameter. Gaps between boards must be at least this wide. JLC/PCBWay: ~1.6–2.0 mm.") { hl = Hl::Mill; }
                        advanced(ui, "adv_tabs", |ui| {
                            ui.horizontal(|ui| {
                                field_label(ui, "Place tabs by");
                                ui.selectable_value(&mut c.tab_mode, TabMode::Count, "Count");
                                ui.selectable_value(&mut c.tab_mode, TabMode::Spacing, "Distance");
                            })
                            .response
                            .on_hover_text("Count: a fixed number of tabs per edge. Distance: one tab every … mm, so long edges get more tabs.");
                            if mm_field(ui, "Tab width left/right", &mut c.tab_width_lr, 0.5..=30.0, "Width of the tabs on the left and right board edges, if it should differ from the top / bottom tabs") { hl = Hl::Tabs; }
                            if c.sep == Separation::Mousebites {
                                if mm_field(ui, "Holes past tab", &mut c.mb_prolong, 0.0..=5.0, "The hole row is continued this far beyond both ends of the tab, so the break does not tear into the board edge") { hl = Hl::Holes; }
                            }
                            if mm_field(ui, "Round inner corners", &mut c.mill_radius, 0.0..=3.0, "Rounds all inner corners of the panel outline with this radius, as the router bit will do (use half the router Ø). 0 = keep sharp corners.") { hl = Hl::Mill; }
                        });
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
                if c.framing != Framing::None && (c.tooling != Corners::None || c.fiducials != Corners::None) {
                    advanced(ui, "adv_tooling", |ui| {
                        if c.tooling != Corners::None {
                            if mm_field(ui, "Holes: from edge", &mut c.tool_edge, 0.0..=30.0, "Hole center distance from the outer panel edge, across the rail. 0 = centered on the rail.") { hl = Hl::Tooling; }
                        }
                        if c.fiducials != Corners::None {
                            if mm_field(ui, "Fiducials: from edge", &mut c.fid_edge, 0.0..=30.0, "Fiducial center distance from the outer panel edge, across the rail. 0 = centered on the rail.") { hl = Hl::Fiducials; }
                        }
                    });
                }
            });

        ui.add_space(6.0);
        egui::CollapsingHeader::new(section_title("Panel text"))
            .default_open(false)
            .show(ui, |ui| {
                section_help(ui, "Up to 4 texts on the rails, e.g. version / date, or JLCJLCJLCJLC to mark where JLC prints its order number.\nPlaceholders: {date} {board} {rows} {cols} {count} {width} {height}");
                let mut remove: Option<usize> = None;
                for (i, t) in c.texts.iter_mut().enumerate() {
                    let r = ui.horizontal(|ui| {
                        field_label(ui, &format!("Text {}", i + 1));
                        if ui.button("🗑").on_hover_text("Remove this text").clicked() {
                            remove = Some(i);
                        }
                        ui.add_sized([ui.available_width() - 6.0, 22.0], egui::TextEdit::singleline(&mut t.text).hint_text("e.g. {board} {date}"))
                    });
                    if r.inner.hovered() || r.inner.has_focus() { hl = Hl::Text; }
                    if mm_field(ui, "Text height", &mut t.size, 0.5..=5.0, "Character height") { hl = Hl::Text; }
                    ui.horizontal(|ui| {
                        field_label(ui, "Rail");
                        egui::ComboBox::from_id_salt(("text_rail", i)).selected_text(t.rail.label()).show_ui(ui, |ui| {
                            for v in TextRail::ALL { ui.selectable_value(&mut t.rail, v, v.label()); }
                        });
                    })
                    .response
                    .on_hover_text("Text on the left / right rail is turned by 90° (reads from bottom to top).");
                    egui::CollapsingHeader::new(egui::RichText::new("More").color(MUTED)).id_salt(("text_more", i)).default_open(false).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            field_label(ui, "Layer");
                            egui::ComboBox::from_id_salt(("text_layer", i)).selected_text(t.layer.clone()).show_ui(ui, |ui| {
                                for l in panel::TEXT_LAYERS { ui.selectable_value(&mut t.layer, l.to_string(), l); }
                            });
                        })
                        .response
                        .on_hover_text("Silkscreen, copper or solder mask. Text on back layers (B.…) is mirrored, so it reads correctly from the back.");
                        ui.horizontal(|ui| {
                            field_label(ui, "Position on rail");
                            ui.selectable_value(&mut t.justify, Justify::Start, "Start");
                            ui.selectable_value(&mut t.justify, Justify::Center, "Center");
                            ui.selectable_value(&mut t.justify, Justify::End, "End");
                        });
                        if mm_field(ui, "Shift along rail", &mut t.offset, -500.0..=500.0, "Start / End: distance from that end of the rail. Center: shift away from the middle.") { hl = Hl::Text; }
                        if mm_field(ui, "Line thickness", &mut t.thickness, 0.0..=1.0, "Stroke width of the letters. 0 = automatic (15 % of the height).") { hl = Hl::Text; }
                    });
                    ui.add_space(4.0);
                }
                if let Some(i) = remove {
                    c.texts.remove(i);
                }
                if c.texts.len() < panel::MAX_TEXTS && ui.button("+  Add text").clicked() {
                    c.texts.push(TextCfg::default());
                }
            });

        ui.add_space(6.0);
        egui::CollapsingHeader::new(section_title("Output file"))
            .default_open(true)
            .show(ui, |ui| {
                section_help(ui, "Where the panel is written. Empty folder = <project>/panel. Relative paths are relative to the project folder. Edge.Cuts becomes the panel outline.");
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
                ui.horizontal(|ui| {
                    field_label(ui, "Panel position");
                    ui.add(egui::DragValue::new(&mut c.origin.0).speed(1.0).prefix("x ").suffix(" mm"));
                    ui.add(egui::DragValue::new(&mut c.origin.1).speed(1.0).prefix("y ").suffix(" mm"));
                }).response.on_hover_text("Top-left corner of the panel on the KiCad page");
                advanced(ui, "adv_output", |ui| {
                    text_field(ui, "Net names", &mut c.net_template, "Board_{n}-{orig}", "Name of each net in the panel. {n} = board number, {orig} = original net name. With {n}, every board keeps its own nets.");
                    text_field(ui, "References", &mut c.ref_template, "{orig}", "Reference designator of each part. {orig} keeps R1 on every board; {orig}_{n} gives R1_1, R1_2, … (needed by some assembly services).");
                    mm_field(ui, "Outline line width", &mut c.edge_width, 0.01..=1.0, "Line width of the panel outline on Edge.Cuts");
                    ui.checkbox(&mut c.page_fit, "Fit page size to the panel")
                        .on_hover_text("Sets a custom page size around the panel, with the panel position as margin. Otherwise the page of your board is kept.");
                    ui.horizontal(|ui| {
                        field_label(ui, "Drill / place origin");
                        ui.selectable_value(&mut c.aux_origin, OriginCorner::None, "Keep");
                        ui.selectable_value(&mut c.aux_origin, OriginCorner::TopLeft, "Top left");
                        ui.selectable_value(&mut c.aux_origin, OriginCorner::BottomLeft, "Bottom left");
                    })
                    .response
                    .on_hover_text("Moves the drill / place file origin and the grid origin to a panel corner. Assembly houses usually want bottom left.");
                });
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

    /// "2D | 3D" switch in the top-left corner of the preview, and the mouse hint at the bottom
    fn view_switch(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, rect: Rect) {
        let hint = if self.view3d {
            "drag = rotate  ·  right-drag or Shift+drag = move  ·  wheel = zoom  ·  double-click = reset"
        } else {
            "drag = pan  ·  wheel = zoom  ·  double-click = fit"
        };
        painter.text(rect.left_bottom() + Vec2::new(10.0, -8.0), egui::Align2::LEFT_BOTTOM, hint, egui::FontId::proportional(12.5), Color32::from_rgb(110, 130, 170));

        let area = Rect::from_min_size(rect.min + Vec2::new(10.0, 10.0), egui::vec2(92.0, 30.0));
        painter.rect_filled(area, 6.0, Color32::from_rgb(27, 30, 36));
        let modes = [
            (false, "2D", "Flat view"),
            (true, "3D", "Rotatable 3D view of the panel with the components of your board. The components are fetched from KiCad once per board, which takes a few seconds; until then the bare panel is shown."),
        ];
        for (i, (is3d, label, tip)) in modes.into_iter().enumerate() {
            let r = Rect::from_min_size(area.min + Vec2::new(3.0 + 44.0 * i as f32, 3.0), egui::vec2(42.0, 24.0));
            if ui.put(r, egui::SelectableLabel::new(self.view3d == is3d, label)).on_hover_text(tip).clicked() {
                if self.view3d != is3d {
                    self.view3d = is3d;
                    self.fit_pending = true;
                }
            }
        }
        if self.view3d {
            let gl_err = self.gl3d.error();
            let (text, col) = match (&self.model, gl_err) {
                (ModelLoad::Loading(_), _) => ("⏳ Fetching the components from KiCad…".to_string(), DIM),
                (ModelLoad::Failed(e), _) => (format!("Components not shown: {}", elide_middle(e.trim_start_matches("kicad-cli failed: "), 110)), WARN_TEXT),
                (_, Some(e)) => (format!("Components not shown (graphics driver): {}", elide_middle(&e, 90)), WARN_TEXT),
                _ => (String::new(), DIM),
            };
            painter.text(area.right_center() + Vec2::new(12.0, 0.0), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(13.5), col);
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
        let feats = panel::features(&p, &self.cfg, &sep.vcuts, &self.board_stem);
        self.warnings = sep.warnings.clone();
        self.warnings.extend(feats.warnings.iter().cloned());
        let (pw, ph) = p.size;

        // exact final outline (corner shapes, rounded inner corners); cached, the union is not free
        let geo: Option<std::rc::Rc<export::OutlineGeo>> = if p.boards.len() <= OUTLINE_PREVIEW_MAX {
            let key = serde_json::to_string(&self.cfg).unwrap_or_default();
            if self.outline_cache.as_ref().map(|c| c.0 != key).unwrap_or(true) {
                self.outline_cache = Some((key, std::rc::Rc::new(export::outline_geo(&p, &sep, &self.cfg))));
            }
            self.outline_cache.as_ref().map(|c| c.1.clone())
        } else {
            None
        };
        let rings = geo.as_ref().map(|g| &g.rings);

        if self.fit_pending {
            // a turned panel needs more room than a flat one
            let margin = if self.view3d { 0.6 } else { 0.85 };
            self.zoom = (rect.width() / pw as f32).min(rect.height() / ph as f32) * margin;
            self.pan = Vec2::ZERO;
            self.fit_pending = false;
        }
        if self.view3d && matches!(self.model, ModelLoad::Idle) && self.source.is_some() {
            self.start_model(ui.ctx());
        }
        let shift = ui.input(|i| i.modifiers.shift);
        if self.view3d && resp.dragged_by(egui::PointerButton::Primary) && !shift {
            self.cam.rotate(resp.drag_delta());
        } else if resp.dragged() {
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
            self.cam = view3d::Camera::default();
        }

        // areas that belong to the hovered setting
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
            Hl::Text => feats.texts.iter().map(|t| t.rect).collect(),
            Hl::Bones => p.bones.clone(),
            Hl::Copper => feats.copper.clone(),
            Hl::HSpace => p.hgaps.clone(),
            Hl::VSpace => p.vgaps.clone(),
            Hl::Rails => p.rails.clone(),
            Hl::RailGap => p.rail_gaps.clone(),
            Hl::Grid => p.boards.iter().map(|b| b.bbox).collect(),
            Hl::None => vec![],
        };
        // DRC findings in panel coordinates (the file is shifted by the panel position)
        let marks: Vec<((f64, f64), bool)> = match (self.show_drc, &self.drc) {
            (true, Some(r)) => r.items.iter().filter_map(|it| it.pos.map(|(x, y)| ((x - self.cfg.origin.0, y - self.cfg.origin.1), it.error))).collect(),
            _ => Vec::new(),
        };

        if self.view3d {
            // with the board model loaded, OpenGL draws the panel body and the boards
            let gl = match (&geo, &self.outline_cache) {
                (Some(_), Some((key, _))) if self.gl3d.has_model() && self.gl3d.error().is_none() => {
                    let view = gl3d::View { cam: &self.cam, zoom: self.zoom, origin: rect.center() + self.pan, rect };
                    let cfg = &self.cfg;
                    Some(self.gl3d.callback(&view, &p, || export::frame_geo(&p, &sep, cfg), &format!("{key}|{}", self.thickness), self.thickness))
                }
                _ => None,
            };
            match &geo {
                Some(g) => view3d::draw(
                    &painter,
                    rect,
                    &view3d::Scene {
                        cam: &self.cam,
                        zoom: self.zoom,
                        origin: rect.center() + self.pan,
                        geo: g,
                        panel: &p,
                        sep: &sep,
                        feats: &feats,
                        cfg: &self.cfg,
                        thickness: self.thickness,
                        hl: &hl_rects,
                        marks: &marks,
                        vcut_bold: self.hl == Hl::VCuts,
                    },
                    gl,
                ),
                None => {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("The 3D view is switched off for panels with more than {OUTLINE_PREVIEW_MAX} boards."),
                        egui::FontId::proportional(15.0),
                        DIM,
                    );
                }
            }
            self.view_switch(ui, &painter, rect);
            return;
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
            if rings.is_none() {
                painter.rect_stroke(srect(r), 0.0, Stroke::new(1.0_f32, EDGE), egui::StrokeKind::Middle);
            }
        }

        // extra substrate: tabs, filled gaps, backbones
        for r in sep.tabs.iter().chain(sep.fills.iter()).chain(p.bones.iter()) {
            painter.rect_filled(srect(r), 0.0, SUBSTRATE);
        }
        // copper fill on the rails
        for r in &feats.copper {
            painter.rect_filled(srect(r), 0.0, COPPER);
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
                if rings.is_none() {
                    painter.add(egui::Shape::closed_line(pts, Stroke::new(1.2_f32, EDGE)));
                }
            }
        }
        // what the fab will cut: the final outline
        for r in rings.into_iter().flatten() {
            let pts: Vec<Pos2> = r.iter().map(|&q| ts(q)).collect();
            painter.add(egui::Shape::closed_line(pts, Stroke::new(1.2_f32, EDGE)));
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
        for t in &feats.texts {
            let col = if t.layer.ends_with(".Cu") {
                Color32::from_rgb(230, 190, 80)
            } else if t.layer.starts_with("B.") {
                Color32::from_rgb(150, 190, 255)
            } else {
                Color32::from_rgb(235, 235, 235)
            };
            let center = srect(&t.rect).center();
            let font = egui::FontId::monospace((t.size as f32 * zoom * 1.1).max(4.0));
            if t.angle == 0.0 {
                painter.text(center, egui::Align2::CENTER_CENTER, &t.text, font, col);
            } else {
                // reads bottom to top; the shape turns around the galley's top-left corner
                let g = painter.layout_no_wrap(t.text.clone(), font, col);
                let (gw, gh) = (g.size().x, g.size().y);
                let pos = Pos2::new(center.x - gh / 2.0, center.y + gw / 2.0);
                painter.add(egui::epaint::TextShape::new(pos, g, col).with_angle(-std::f32::consts::FRAC_PI_2));
            }
        }

        // highlight
        for r in &hl_rects {
            let sr = srect(r);
            // make zero-width gaps visible
            let sr = if sr.width() < 2.0 { sr.expand2(Vec2::new(2.0, 0.0)) } else { sr };
            let sr = if sr.height() < 2.0 { sr.expand2(Vec2::new(0.0, 2.0)) } else { sr };
            painter.rect_filled(sr, 0.0, HL);
            painter.rect_stroke(sr, 0.0, Stroke::new(1.0_f32, HL_STROKE), egui::StrokeKind::Middle);
        }

        // DRC findings
        for &(q, error) in &marks {
            let c = ts(q);
            let col = if error { BAD } else { WARN_TEXT };
            painter.circle_stroke(c, 7.0, Stroke::new(2.0_f32, col));
            painter.line_segment([c - Vec2::splat(4.0), c + Vec2::splat(4.0)], Stroke::new(2.0_f32, col));
            painter.line_segment([c + Vec2::new(-4.0, 4.0), c + Vec2::new(4.0, -4.0)], Stroke::new(2.0_f32, col));
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
        self.view_switch(ui, &painter, rect);
        let _ = p.area;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_job(ctx);
        self.poll_model();

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

                        // what the last automatic fix changed, with a way back
                        let mut undo = false;
                        let mut dismiss = false;
                        if let Some((changes, _)) = &self.fix_log {
                            egui::Frame::NONE.fill(Color32::from_rgb(22, 50, 34)).corner_radius(4.0).inner_margin(8.0).show(ui, |ui| {
                                ui.set_width(ui.available_width() - 4.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(format!("🔧 Adjusted {} setting(s)", changes.len())).strong().color(OK_TEXT));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        dismiss = ui.small_button("OK").on_hover_text("Keep the changes and hide this list").clicked();
                                        undo = ui.small_button("Undo").on_hover_text("Put all settings back as they were before the fix").clicked();
                                    });
                                });
                                egui::ScrollArea::vertical().id_salt("fixlog").max_height(96.0).auto_shrink([false, true]).show(ui, |ui| {
                                    for c in changes {
                                        ui.label(egui::RichText::new(format!("• {c}")).small().color(Color32::from_rgb(190, 225, 195)));
                                    }
                                });
                            });
                            ui.add_space(6.0);
                        }
                        if undo {
                            if let Some((_, before)) = self.fix_log.take() {
                                self.cfg = before;
                                self.notice = Notice::ok("✔ Automatic fix undone");
                                self.fit_pending = true;
                            }
                        } else if dismiss {
                            self.fix_log = None;
                        }

                        // checks
                        let mut fix = false;
                        if !self.warnings.is_empty() {
                            let title = if errors > 0 {
                                egui::RichText::new(format!("⚠ Checks — {errors} problem(s), {notes} note(s)")).color(ERR_TEXT)
                            } else {
                                egui::RichText::new(format!("Checks — {notes} note(s)")).color(WARN_TEXT)
                            };
                            ui.horizontal(|ui| {
                                ui.label(title.strong().size(15.5));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    fix = ui
                                        .button("🔧  Fix automatically")
                                        .on_hover_text("Adjusts the settings so the problems below go away: wider gaps or rails, narrower tabs, moved holes, fiducials and texts, …\nYou see what was changed and can undo it.")
                                        .clicked();
                                });
                            });
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
                        if fix {
                            self.auto_fix();
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

                        // tools that run KiCad's own checks / 3D export on the exported panel file
                        ui.add_space(8.0);
                        if let Some((job, _)) = &self.job {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(egui::RichText::new(job.label()).color(MUTED));
                            });
                        } else {
                            let ready = has_board && self.panel_file().is_some();
                            let mut start: Option<tools::Job> = None;
                            ui.horizontal(|ui| {
                                let w = (ui.available_width() - 2.0 * 8.0) / 3.0;
                                let hint = "Export the panel first";
                                let cell = egui::Layout::top_down_justified(egui::Align::Center);
                                ui.add_enabled_ui(ready, |ui| {
                                    if ui
                                        .add(egui::Button::new("✔  Check (DRC)").min_size(egui::vec2(w, 32.0)))
                                        .on_hover_text("Run KiCad's design rule check on the exported panel and list what it finds.\nIt checks the exported file: export again after changing settings.")
                                        .on_disabled_hover_text(hint)
                                        .clicked()
                                    {
                                        start = Some(tools::Job::Drc);
                                    }
                                    ui.allocate_ui_with_layout(egui::vec2(w, 32.0), cell, |ui| {
                                        ui.menu_button("3D picture  ⏷", |ui| {
                                            for v in tools::View::ALL {
                                                if ui.button(v.label()).clicked() {
                                                    start = Some(tools::Job::Render { view: v, high: self.cfg.render_high });
                                                    ui.close_menu();
                                                }
                                            }
                                            ui.separator();
                                            ui.checkbox(&mut self.cfg.render_high, "High quality (slow)");
                                        })
                                        .response
                                        .on_hover_text("Render the exported panel in 3D and save it as a PNG next to the panel file")
                                        .on_disabled_hover_text(hint);
                                    });
                                    ui.allocate_ui_with_layout(egui::vec2(w, 32.0), cell, |ui| {
                                        ui.menu_button("3D model  ⏷", |ui| {
                                            for m in tools::Model::ALL {
                                                if ui.button(m.label()).clicked() {
                                                    start = Some(tools::Job::Model(m));
                                                    ui.close_menu();
                                                }
                                            }
                                        })
                                        .response
                                        .on_hover_text("Export the panel as a 3D model file next to the panel file. Large panels can take a few minutes.")
                                        .on_disabled_hover_text(hint);
                                    });
                                });
                            });
                            if let Some(j) = start {
                                self.start_job(ctx, j);
                            }
                        }
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
        self.drc_window(ctx);
        self.render_window(ctx);
        self.save_render_copy();

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
        // the 3D view needs a depth buffer; 4 samples smooth its edges
        depth_buffer: 24,
        multisampling: 4,
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
