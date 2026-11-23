//! Rotatable 3D view of the bare panel, drawn with the normal 2D painter (no GPU code).
//!
//! The panel is a flat slab, which keeps hidden-surface removal simple: side walls that face the
//! camera are drawn back to front, then the visible face (top or bottom) on top of them, then
//! everything that lies on that face (copper, holes, fiducials, text, V-cuts).

use crate::export::OutlineGeo;
use crate::panel::{FeatureKind, Features, Panel, PanelConfig, RectMm, Sep};
use crate::{BAD, BG, BOARD, COPPER, DIM, EDGE, HL, HL_STROKE, VCUT, WARN_TEXT};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

type Pt = (f64, f64);

/// viewing direction; zoom and pan are shared with the 2D view
pub struct Camera {
    /// rotation around the vertical axis (radians)
    pub yaw: f32,
    /// elevation: 90° = straight from above, 0 = from the side, negative = from below
    pub pitch: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { yaw: -0.45, pitch: 0.85 }
    }
}

impl Camera {
    pub fn rotate(&mut self, drag: Vec2) {
        self.yaw += drag.x * 0.01;
        self.pitch = (self.pitch + drag.y * 0.01).clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
    }
}

pub struct Scene<'a> {
    pub cam: &'a Camera,
    /// pixels per mm
    pub zoom: f32,
    /// screen position of the panel center
    pub origin: Pos2,
    pub geo: &'a OutlineGeo,
    pub panel: &'a Panel,
    pub sep: &'a Sep,
    pub feats: &'a Features,
    pub cfg: &'a PanelConfig,
    /// board thickness (mm)
    pub thickness: f64,
    /// highlighted areas (hovered setting)
    pub hl: &'a [RectMm],
    /// DRC findings in panel coordinates: (position, is error)
    pub marks: &'a [(Pt, bool)],
    pub vcut_bold: bool,
}

/// panel coordinates (mm, y down) + height -> screen
struct Proj {
    sy: f32,
    cy: f32,
    sp: f32,
    cp: f32,
    zoom: f32,
    origin: Pos2,
    center: Pt,
}

impl Proj {
    /// position turned around the vertical axis: (right, away-from-camera along the ground)
    fn turned(&self, q: Pt) -> (f32, f32) {
        let x = (q.0 - self.center.0) as f32;
        let y = -((q.1 - self.center.1) as f32); // y up
        (x * self.cy - y * self.sy, x * self.sy + y * self.cy)
    }
    fn at(&self, q: Pt, z: f32) -> Pos2 {
        let (x1, y1) = self.turned(q);
        let up = y1 * self.sp + z * self.cp;
        self.origin + Vec2::new(x1 * self.zoom, -up * self.zoom)
    }
    /// larger = closer to the camera (same height assumed)
    fn depth(&self, q: Pt) -> f32 {
        -self.turned(q).1 * self.cp
    }
}

fn shade(c: Color32, f: f32) -> Color32 {
    let m = |v: u8| (v as f32 * f).clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

/// `gl`: paint job that draws the panel body and the board models (see `gl3d`); without it the
/// bare panel is drawn here.
pub fn draw(painter: &egui::Painter, rect: Rect, s: &Scene, gl: Option<egui::PaintCallback>) {
    let (pw, ph) = s.panel.size;
    let pr = Proj {
        sy: s.cam.yaw.sin(),
        cy: s.cam.yaw.cos(),
        sp: s.cam.pitch.sin(),
        cp: s.cam.pitch.cos(),
        zoom: s.zoom,
        origin: s.origin,
        center: (pw / 2.0, ph / 2.0),
    };
    let half = (s.thickness / 2.0) as f32;
    // the face we look at
    let top = pr.sp >= 0.0;
    let zf = if top { half } else { -half };
    let on = |q: Pt| pr.at(q, zf);

    let solid = gl.is_some();
    if let Some(cb) = gl {
        painter.add(cb);
    }
    if !solid {
    // ---- side walls facing the camera, far ones first
    let wall_col = Color32::from_rgb(168, 160, 104); // bare FR4 edge
    let mut walls: Vec<(f32, [Pos2; 4], Color32)> = Vec::new();
    for &(a, b) in &s.geo.walls {
        let (dx, dy) = ((b.0 - a.0) as f32, (b.1 - a.1) as f32);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-6 {
            continue;
        }
        // outward normal (y up), then turned with the camera
        let (nx, ny) = (dy / len, dx / len);
        let (n1x, n1y) = (nx * pr.cy - ny * pr.sy, nx * pr.sy + ny * pr.cy);
        if n1y >= 0.0 {
            continue; // faces away
        }
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let col = shade(wall_col, 0.55 + 0.35 * -n1y + 0.12 * n1x);
        walls.push((pr.depth(mid), [pr.at(a, half), pr.at(b, half), pr.at(b, -half), pr.at(a, -half)], col));
    }
    walls.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut mesh = egui::Mesh::default();
    for (_, q, col) in &walls {
        let i = mesh.vertices.len() as u32;
        for p in q {
            mesh.colored_vertex(*p, *col);
        }
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    painter.add(egui::Shape::mesh(mesh));

    // ---- visible face
    let face_col = if top { BOARD } else { shade(BOARD, 0.8) };
    let mut mesh = egui::Mesh::default();
    for &v in &s.geo.verts {
        mesh.colored_vertex(on(v), face_col);
    }
    for t in &s.geo.tris {
        mesh.add_triangle(t[0], t[1], t[2]);
    }
    painter.add(egui::Shape::mesh(mesh));
    for r in &s.geo.rings {
        painter.add(egui::Shape::closed_line(r.iter().map(|&q| on(q)).collect(), Stroke::new(1.0_f32, EDGE)));
    }
    }

    // ---- things on the face
    let quad = |r: &RectMm| vec![on(r.min), on((r.max.0, r.min.1)), on(r.max), on((r.min.0, r.max.1))];
    let disc = |c: Pt, r: f64, n: usize| -> Vec<Pos2> {
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                on((c.0 + r * a.cos(), c.1 + r * a.sin()))
            })
            .collect()
    };
    for r in &s.feats.copper {
        painter.add(egui::Shape::convex_polygon(quad(r), COPPER, Stroke::NONE));
    }
    // holes go through: the background shows
    for &(c, d) in &s.sep.holes {
        painter.add(egui::Shape::convex_polygon(disc(c, d / 2.0, 10), BG, Stroke::NONE));
    }
    for f in &s.feats.items {
        match f.kind {
            FeatureKind::Tooling => {
                painter.add(egui::Shape::convex_polygon(disc(f.pos, f.r, 24), BG, Stroke::new(1.0_f32, EDGE)));
            }
            FeatureKind::FiducialTop | FeatureKind::FiducialBottom => {
                if (f.kind == FeatureKind::FiducialTop) != top {
                    continue; // on the other side
                }
                painter.add(egui::Shape::convex_polygon(disc(f.pos, s.cfg.fid_opening / 2.0, 24), Color32::from_rgb(15, 45, 30), Stroke::NONE));
                painter.add(egui::Shape::convex_polygon(disc(f.pos, s.cfg.fid_copper / 2.0, 16), Color32::from_rgb(230, 190, 80), Stroke::NONE));
            }
        }
    }
    for &(a, b) in &s.sep.breaks {
        painter.add(egui::Shape::dashed_line(&[on(a), on(b)], Stroke::new(1.5_f32, Color32::WHITE), 4.0, 3.0));
    }
    let vw = if s.vcut_bold { 2.5_f32 } else { 1.5_f32 };
    for v in &s.sep.vcuts {
        let (a, b) = if v.vertical { ((v.pos, 0.0), (v.pos, ph)) } else { ((0.0, v.pos), (pw, v.pos)) };
        painter.add(egui::Shape::dashed_line(&[on(a), on(b)], Stroke::new(vw, if v.conflict { BAD } else { VCUT }), 8.0, 4.0));
    }
    // texts: only those printed on the side we look at, lying flat along their baseline
    for t in &s.feats.texts {
        let back = t.layer.starts_with("B.");
        if back == top {
            continue;
        }
        let (cx, cy) = ((t.rect.min.0 + t.rect.max.0) / 2.0, (t.rect.min.1 + t.rect.max.1) / 2.0);
        // baseline from the first to the last letter; back-side text is mirrored
        let (a, b) = if t.angle == 0.0 {
            let (l, r) = ((t.rect.min.0, cy), (t.rect.max.0, cy));
            if back { (r, l) } else { (l, r) }
        } else {
            ((cx, t.rect.max.1), (cx, t.rect.min.1))
        };
        let d = on(b) - on(a);
        let mm = (((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt() as f32).max(1e-3);
        // shrink with the foreshortening of the baseline
        let px = t.size as f32 * 1.1 * d.length() / mm;
        if px < 3.0 {
            continue;
        }
        let col = if t.layer.ends_with(".Cu") { Color32::from_rgb(230, 190, 80) } else { Color32::from_rgb(235, 235, 235) };
        let g = painter.layout_no_wrap(t.text.clone(), egui::FontId::monospace(px), col);
        let ang = d.y.atan2(d.x);
        let (sa, ca) = ang.sin_cos();
        let (gw, gh) = (g.size().x / 2.0, g.size().y / 2.0);
        // the shape turns around its top-left corner: place that so the middle lands on the text center
        let pos = on((cx, cy)) - Vec2::new(gw * ca - gh * sa, gw * sa + gh * ca);
        painter.add(egui::epaint::TextShape::new(pos, g, col).with_angle(ang));
    }
    for r in s.hl {
        // keep zero-width gaps visible
        let r = RectMm::new(r.min.0, r.min.1, r.max.0.max(r.min.0 + 0.3), r.max.1.max(r.min.1 + 0.3));
        painter.add(egui::Shape::convex_polygon(quad(&r), HL, Stroke::new(1.0_f32, HL_STROKE)));
    }
    for &(q, error) in s.marks {
        let c = on(q);
        let col = if error { BAD } else { WARN_TEXT };
        painter.circle_stroke(c, 7.0, Stroke::new(2.0_f32, col));
        painter.line_segment([c - Vec2::splat(4.0), c + Vec2::splat(4.0)], Stroke::new(2.0_f32, col));
        painter.line_segment([c + Vec2::new(-4.0, 4.0), c + Vec2::new(4.0, -4.0)], Stroke::new(2.0_f32, col));
    }

    painter.text(
        rect.center_bottom() + Vec2::new(0.0, -30.0),
        egui::Align2::CENTER_CENTER,
        format!(
            "Panel {:.2} × {:.2} × {:.2} mm  ·  {} boards  ·  {}",
            pw,
            ph,
            s.thickness,
            s.panel.boards.len(),
            if top { "top side" } else { "bottom side" }
        ),
        egui::FontId::proportional(15.0),
        DIM,
    );
}
