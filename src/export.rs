//! Write the panel as a new .kicad_pcb file.
//!
//! - every board copy is cloned from the source S-expression, transformed (rotation + shift),
//!   gets fresh UUIDs and per-copy net names (Board_N-<net>)
//! - original Edge.Cuts are removed; the panel outline is the union of boards, rails, tabs, fills
//! - V-cut lines, mousebite / tooling NPTH holes, fiducials and text are added

use crate::panel::{apply_template, CornerStyle, Feature, FeatureKind, Features, Framing, Justify, OriginCorner, Panel, PanelConfig, PanelText, Sep};
use crate::sexpr::{Sexp, fmt_num};
use geo::{unary_union, Coord, LineString, MultiPolygon, Polygon};
use std::collections::HashMap;

type Pt = (f64, f64);

const HEADER: &[&str] = &[
    "version",
    "generator",
    "generator_version",
    "general",
    "paper",
    "title_block",
    "layers",
    "setup",
    "property",
    "embedded_fonts",
    "embedded_files",
];
const DROP: &[&str] = &["group", "generated"];
const POINT_NODES: &[&str] = &["start", "end", "mid", "center", "xy", "at"];

fn new_uuid() -> Sexp {
    Sexp::str(&uuid::Uuid::new_v4().to_string())
}

fn on_layer(n: &Sexp, layer: &str) -> bool {
    n.child("layer").and_then(|l| l.atom(1)) == Some(layer)
}

struct Xform<'a> {
    panel: &'a Panel,
    idx: usize,
    origin: Pt,
}

impl Xform<'_> {
    fn pt(&self, p: Pt) -> Pt {
        let q = self.panel.to_panel(self.idx, p);
        (q.0 + self.origin.0, q.1 + self.origin.1)
    }
    fn rot(&self) -> f64 {
        self.panel.boards[self.idx].rotation as f64
    }
}

fn norm_angle(a: f64) -> f64 {
    let a = a % 360.0;
    if a < 0.0 { a + 360.0 } else { a }
}

/// add `r` degrees to the angle of an (at x y [angle]) node
fn rotate_at(at: &mut Sexp, r: f64) {
    if r == 0.0 {
        return;
    }
    let a = at.f64_at(3).unwrap_or(0.0);
    let new = Sexp::num(norm_angle(a + r));
    if at.children().len() > 3 && at.f64_at(3).is_some() {
        at.set(3, new);
    } else if let Some(v) = at.children_mut() {
        // keep possible trailing atoms like "unlocked"
        v.insert(3.min(v.len()), new);
    }
}

/// transform every absolute point in the subtree
fn transform_abs(n: &mut Sexp, xf: &Xform) {
    if let Some(name) = n.name().map(|s| s.to_string()) {
        if POINT_NODES.contains(&name.as_str()) {
            if let (Some(x), Some(y)) = (n.f64_at(1), n.f64_at(2)) {
                let (nx, ny) = xf.pt((x, y));
                n.set(1, Sexp::num(nx));
                n.set(2, Sexp::num(ny));
                if name == "at" {
                    rotate_at(n, xf.rot());
                }
                return;
            }
        }
    }
    if let Some(v) = n.children_mut() {
        for c in v.iter_mut() {
            transform_abs(c, xf);
        }
    }
}

/// footprints: only the footprint position is absolute; child angles are absolute too
fn transform_footprint(fp: &mut Sexp, xf: &Xform) {
    let r = xf.rot();
    let Some(children) = fp.children_mut() else { return };
    children.retain(|c| {
        !(matches!(c.name(), Some("fp_line" | "fp_arc" | "fp_circle" | "fp_rect" | "fp_poly")) && on_layer(c, "Edge.Cuts"))
    });
    for c in children.iter_mut() {
        match c.name() {
            Some("at") => {
                if let (Some(x), Some(y)) = (c.f64_at(1), c.f64_at(2)) {
                    let (nx, ny) = xf.pt((x, y));
                    c.set(1, Sexp::num(nx));
                    c.set(2, Sexp::num(ny));
                    rotate_at(c, r);
                }
            }
            Some("pad" | "property" | "fp_text" | "fp_text_box") => {
                if let Some(v) = c.children_mut() {
                    for at in v.iter_mut().filter(|x| x.name() == Some("at")) {
                        rotate_at(at, r);
                    }
                }
            }
            // zones inside footprints use absolute board coordinates
            Some("zone") => transform_abs(c, xf),
            _ => {}
        }
    }
}

/// fresh UUIDs everywhere in the subtree
fn renew_uuids(n: &mut Sexp) {
    if matches!(n.name(), Some("uuid" | "tstamp")) {
        n.set(1, new_uuid());
        return;
    }
    if let Some(v) = n.children_mut() {
        for c in v.iter_mut() {
            renew_uuids(c);
        }
    }
}

struct NetMap<'a> {
    /// old net number -> name
    names: HashMap<i64, String>,
    stride: i64,
    /// net name pattern with {n} and {orig}
    tpl: &'a str,
}

fn rename_net(name: &str, copy: usize, tpl: &str) -> String {
    if name.is_empty() { String::new() } else { escape_raw(&apply_template(tpl, copy + 1, name)) }
}

/// names are kept raw (already escaped) by the parser; only escape quotes a pattern could add
fn escape_raw(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut prev_bs = false;
    for c in s.chars() {
        if c == '"' && !prev_bs {
            o.push(BACKSLASH);
        }
        prev_bs = c == BACKSLASH && !prev_bs;
        o.push(c);
    }
    o
}

const BACKSLASH: char = 92 as char;

fn remap_nets(n: &mut Sexp, copy: usize, nm: &NetMap) {
    match n.name() {
        Some("net") => {
            // (net 3 "GND") | (net 3) | (net "GND")
            if let Some(id) = n.atom(1).and_then(|s| s.parse::<i64>().ok()).filter(|_| !n.is_quoted(1)) {
                if id > 0 {
                    n.set(1, Sexp::sym(&(id + copy as i64 * nm.stride).to_string()));
                }
                if n.children().len() > 2 && n.is_quoted(2) {
                    let old = n.atom(2).unwrap_or("").to_string();
                    n.set(2, Sexp::Atom(rename_net(&old, copy, nm.tpl), true));
                }
            } else if n.is_quoted(1) {
                let old = n.atom(1).unwrap_or("").to_string();
                n.set(1, Sexp::Atom(rename_net(&old, copy, nm.tpl), true));
            }
            return;
        }
        Some("net_name") if n.is_quoted(1) => {
            let old = n.atom(1).unwrap_or("").to_string();
            n.set(1, Sexp::Atom(rename_net(&old, copy, nm.tpl), true));
            return;
        }
        _ => {}
    }
    if let Some(v) = n.children_mut() {
        for c in v.iter_mut() {
            remap_nets(c, copy, nm);
        }
    }
    let _ = &nm.names;
}

fn rename_ref(fp: &mut Sexp, copy: usize, tpl: &str) {
    if let Some(v) = fp.children_mut() {
        for c in v.iter_mut() {
            let is_ref = (c.name() == Some("property") && c.atom(1) == Some("Reference"))
                || (c.name() == Some("fp_text") && c.atom(1) == Some("reference"));
            if is_ref {
                let old = c.atom(2).unwrap_or("").to_string();
                c.set(2, Sexp::Atom(escape_raw(&apply_template(tpl, copy + 1, &old)), true));
            }
        }
    }
}

// ------------------------------------------------------------- panel primitives

fn xy(p: Pt) -> Vec<Sexp> {
    vec![Sexp::num(p.0), Sexp::num(p.1)]
}

fn node(name: &str, mut args: Vec<Sexp>) -> Sexp {
    let mut v = vec![Sexp::sym(name)];
    v.append(&mut args);
    Sexp::list(v)
}

fn effects(size: f64) -> Sexp {
    node(
        "effects",
        vec![node(
            "font",
            vec![
                node("size", vec![Sexp::num(size), Sexp::num(size)]),
                node("thickness", vec![Sexp::num((size * 0.15).max(0.1))]),
            ],
        )],
    )
}

/// syntax differences between KiCad file versions, detected from the source file
#[derive(Clone, Copy)]
struct Style {
    /// `(hide yes)` (KiCad 8+) vs bare `hide` atom (KiCad 7)
    hide_list: bool,
    /// `(fill no)` (KiCad 9) vs `(fill none)` (KiCad 7/8)
    fill_none: bool,
}

thread_local! {
    static STYLE: std::cell::Cell<Style> = const { std::cell::Cell::new(Style { hide_list: true, fill_none: false }) };
}

fn style() -> Style {
    STYLE.with(|s| s.get())
}

fn hidden_prop(name: &str, val: &str, layer: &str) -> Sexp {
    let hide = if style().hide_list { node("hide", vec![Sexp::sym("yes")]) } else { Sexp::sym("hide") };
    node(
        "property",
        vec![
            Sexp::str(name),
            Sexp::str(val),
            node("at", vec![Sexp::num(0.0), Sexp::num(0.0), Sexp::num(0.0)]),
            node("layer", vec![Sexp::str(layer)]),
            hide,
            node("uuid", vec![new_uuid()]),
            effects(1.0),
        ],
    )
}

/// minimal board-only footprint with one pad
/// `lib` has no library prefix on purpose: KiCad's DRC reports every footprint whose library is not
/// installed, which would flood the report with one warning per mousebite
fn footprint(lib: &str, reference: &str, value: &str, back: bool, at: Pt, pad: Sexp) -> Sexp {
    let (cu, silk, fab) = if back { ("B.Cu", "B.SilkS", "B.Fab") } else { ("F.Cu", "F.SilkS", "F.Fab") };
    node(
        "footprint",
        vec![
            Sexp::str(lib),
            node("layer", vec![Sexp::str(cu)]),
            node("uuid", vec![new_uuid()]),
            node("at", xy(at)),
            hidden_prop("Reference", reference, silk),
            hidden_prop("Value", value, fab),
            node(
                "attr",
                vec![Sexp::sym("board_only"), Sexp::sym("exclude_from_pos_files"), Sexp::sym("exclude_from_bom")],
            ),
            pad,
        ],
    )
}

fn npth_pad(d: f64) -> Sexp {
    node(
        "pad",
        vec![
            Sexp::str(""),
            Sexp::sym("np_thru_hole"),
            Sexp::sym("circle"),
            node("at", vec![Sexp::num(0.0), Sexp::num(0.0)]),
            node("size", vec![Sexp::num(d), Sexp::num(d)]),
            node("drill", vec![Sexp::num(d)]),
            node("layers", vec![Sexp::str("*.Cu"), Sexp::str("*.Mask")]),
            node("uuid", vec![new_uuid()]),
        ],
    )
}

fn pth_pad(d: f64) -> Sexp {
    node(
        "pad",
        vec![
            Sexp::str("1"),
            Sexp::sym("thru_hole"),
            Sexp::sym("circle"),
            node("at", vec![Sexp::num(0.0), Sexp::num(0.0)]),
            node("size", vec![Sexp::num(d + 0.6), Sexp::num(d + 0.6)]),
            node("drill", vec![Sexp::num(d)]),
            node("layers", vec![Sexp::str("*.Cu"), Sexp::str("*.Mask")]),
            node("uuid", vec![new_uuid()]),
        ],
    )
}

fn fid_pad(copper: f64, opening: f64, back: bool) -> Sexp {
    let (cu, mask) = if back { ("B.Cu", "B.Mask") } else { ("F.Cu", "F.Mask") };
    node(
        "pad",
        vec![
            Sexp::str("1"),
            Sexp::sym("smd"),
            Sexp::sym("circle"),
            node("at", vec![Sexp::num(0.0), Sexp::num(0.0)]),
            node("size", vec![Sexp::num(copper), Sexp::num(copper)]),
            node("layers", vec![Sexp::str(cu), Sexp::str(mask)]),
            node("solder_mask_margin", vec![Sexp::num(((opening - copper) / 2.0).max(0.0))]),
            node("uuid", vec![new_uuid()]),
        ],
    )
}

fn gr_line(a: Pt, b: Pt, layer: &str, width: f64) -> Sexp {
    node(
        "gr_line",
        vec![
            node("start", xy(a)),
            node("end", xy(b)),
            node("stroke", vec![node("width", vec![Sexp::num(width)]), node("type", vec![Sexp::sym("solid")])]),
            node("layer", vec![Sexp::str(layer)]),
            node("uuid", vec![new_uuid()]),
        ],
    )
}

fn gr_poly(ring: &[Pt], layer: &str, width: f64, filled: bool) -> Sexp {
    let pts: Vec<Sexp> = ring.iter().map(|&p| node("xy", xy(p))).collect();
    // KiCad 9: (fill yes|no), KiCad 7/8: (fill solid|none)
    let fill = match (filled, style().fill_none) {
        (true, true) => "solid",
        (true, false) => "yes",
        (false, true) => "none",
        (false, false) => "no",
    };
    node(
        "gr_poly",
        vec![
            node("pts", pts),
            node("stroke", vec![node("width", vec![Sexp::num(width)]), node("type", vec![Sexp::sym("solid")])]),
            node("fill", vec![Sexp::sym(fill)]),
            node("layer", vec![Sexp::str(layer)]),
            node("uuid", vec![new_uuid()]),
        ],
    )
}

fn gr_text(text: &str, at: Pt, size: f64, layer: &str) -> Sexp {
    node(
        "gr_text",
        vec![
            Sexp::str(text),
            node("at", vec![Sexp::num(at.0), Sexp::num(at.1), Sexp::num(0.0)]),
            node("layer", vec![Sexp::str(layer)]),
            node("uuid", vec![new_uuid()]),
            effects(size),
        ],
    )
}

/// panel text with angle, justification and layer; back layers are mirrored
fn panel_text(t: &PanelText, at: Pt) -> Sexp {
    let back = t.layer.starts_with("B.");
    let thickness = if t.thickness > 0.0 { t.thickness } else { (t.size * 0.15).max(0.1) };
    let mut eff = vec![node(
        "font",
        vec![node("size", vec![Sexp::num(t.size), Sexp::num(t.size)]), node("thickness", vec![Sexp::num(thickness)])],
    )];
    // mirrored text grows the other way from its anchor, so swap the side to keep the position
    let side = match (t.justify, back) {
        (Justify::Center, _) => None,
        (Justify::Start, false) | (Justify::End, true) => Some("left"),
        (Justify::End, false) | (Justify::Start, true) => Some("right"),
    };
    let mut j: Vec<Sexp> = side.into_iter().map(Sexp::sym).collect();
    if back {
        j.push(Sexp::sym("mirror"));
    }
    if !j.is_empty() {
        eff.push(node("justify", j));
    }
    node(
        "gr_text",
        vec![
            Sexp::str(&t.text),
            node("at", vec![Sexp::num(at.0), Sexp::num(at.1), Sexp::num(t.angle)]),
            node("layer", vec![Sexp::str(&t.layer)]),
            node("uuid", vec![new_uuid()]),
            node("effects", eff),
        ],
    )
}

// ------------------------------------------------------------- outline union

fn ring_to_ls(r: &[Pt]) -> LineString<f64> {
    let mut v: Vec<Coord<f64>> = r.iter().map(|&(x, y)| Coord { x, y }).collect();
    if let (Some(f), Some(l)) = (v.first().copied(), v.last().copied()) {
        if f != l {
            v.push(f);
        }
    }
    LineString::new(v)
}

fn rect_poly(r: &crate::panel::RectMm) -> Polygon<f64> {
    Polygon::new(
        ring_to_ls(&[r.min, (r.max.0, r.min.1), r.max, (r.min.0, r.max.1)]),
        vec![],
    )
}

/// panel rectangle with chamfered / rounded corners
fn corner_shape(w: f64, h: f64, style: CornerStyle, size: f64) -> Polygon<f64> {
    let r = size.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut pts: Vec<Pt> = Vec::new();
    // corner point, direction back along the incoming edge, direction along the outgoing edge
    let corners = [((0.0, 0.0), (0.0, 1.0), (1.0, 0.0)), ((w, 0.0), (-1.0, 0.0), (0.0, 1.0)), ((w, h), (0.0, -1.0), (-1.0, 0.0)), ((0.0, h), (1.0, 0.0), (0.0, -1.0))];
    for (c, din, dout) in corners {
        let a: Pt = (c.0 + din.0 * r, c.1 + din.1 * r);
        let b: Pt = (c.0 + dout.0 * r, c.1 + dout.1 * r);
        pts.push(a);
        if style == CornerStyle::Fillet {
            // quarter circle around the center m, from a to b
            let m = (c.0 + (din.0 + dout.0) * r, c.1 + (din.1 + dout.1) * r);
            let n = 12;
            for i in 1..n {
                let t = std::f64::consts::FRAC_PI_2 * i as f64 / n as f64;
                pts.push((m.0 - dout.0 * r * t.cos() - din.0 * r * t.sin(), m.1 - dout.1 * r * t.cos() - din.1 * r * t.sin()));
            }
        }
        pts.push(b);
    }
    Polygon::new(ring_to_ls(&pts), vec![])
}

pub fn panel_outline(p: &Panel, sep: &Sep, cfg: &PanelConfig) -> MultiPolygon<f64> {
    use geo::orient::{Direction, Orient};
    use geo::{Area, BooleanOps, Buffer};
    let mut polys: Vec<Polygon<f64>> = Vec::new();
    for b in &p.boards {
        if let Some(outer) = b.loops.first() {
            let holes = b.loops.iter().skip(1).map(|h| ring_to_ls(h)).collect();
            polys.push(Polygon::new(ring_to_ls(outer), holes));
        }
    }
    for r in p.rails.iter().chain(sep.tabs.iter()).chain(sep.fills.iter()).chain(p.bones.iter()) {
        if r.w() > 1e-6 && r.h() > 1e-6 {
            polys.push(rect_poly(r));
        }
    }
    // consistent winding (exterior CCW, holes CW) — mixed winding breaks the union
    let polys: Vec<Polygon<f64>> = polys.into_iter().map(|p| p.orient(Direction::Default)).collect();
    let mut mp = unary_union(polys.iter());

    // router radius: inner corners cannot be sharper than the milling bit. Growing and shrinking the
    // outline fills them; only the material this adds outside the boards is kept, so the boards' own
    // shape (notches, cutouts, corners) is never touched. The radius stays below half the narrowest
    // slot, otherwise whole slots would close.
    let min_slot = p.slots(cfg).into_iter().filter(|g| *g > 1e-6).fold(f64::MAX, f64::min);
    let r = cfg.mill_radius.min(min_slot / 2.0 * 0.98);
    if r > 1e-3 && cfg.sep != crate::panel::Separation::VCut {
        let boards: Vec<Polygon<f64>> = p.boards.iter().map(|b| rect_poly(&b.bbox).orient(Direction::Default)).collect();
        let added = mp.buffer(r).buffer(-r).difference(&mp).difference(&unary_union(boards.iter()));
        // a real corner fill has an area of about 0.2 r²; anything much smaller is rounding noise
        let fills: Vec<Polygon<f64>> = added.0.into_iter().filter(|a| a.unsigned_area() > 0.02 * r * r).collect();
        mp = unary_union(mp.0.iter().chain(fills.iter()));
    }
    // chamfer / round the outer panel corners (they are rail corners, so only with rails)
    if cfg.corner != CornerStyle::Square && cfg.framing != Framing::None && cfg.corner_size > 1e-6 {
        let shape = corner_shape(p.size.0, p.size.1, cfg.corner, cfg.corner_size).orient(Direction::Default);
        mp = mp.intersection(&shape);
    }
    mp
}

/// final panel outline for the preview, in panel coordinates
pub struct OutlineGeo {
    /// outline rings (outer edges and cutouts), for drawing the cut lines
    pub rings: Vec<Vec<Pt>>,
    /// the panel area as triangles: vertices + index triples (3D view: top / bottom face)
    pub verts: Vec<Pt>,
    pub tris: Vec<[u32; 3]>,
    /// every outline edge a -> b with the material on its left as stored, i.e. the outward
    /// side is (dy, -dx) (3D view: side walls)
    pub walls: Vec<(Pt, Pt)>,
}

pub fn outline_geo(p: &Panel, sep: &Sep, cfg: &PanelConfig) -> OutlineGeo {
    geo_of(panel_outline(p, sep, cfg))
}

/// The panel without the boards: rails, tabs and backbones only. The 3D view draws the boards
/// themselves from their own model, with holes and cutouts.
pub fn frame_geo(p: &Panel, sep: &Sep, cfg: &PanelConfig) -> OutlineGeo {
    use geo::{Area, BooleanOps};
    let boards: Vec<geo::Polygon<f64>> = p
        .boards
        .iter()
        .filter_map(|b| b.loops.first())
        .map(|l| geo::Polygon::new(geo::LineString::from(l.clone()), vec![]))
        .collect();
    let rest = panel_outline(p, sep, cfg).difference(&geo::MultiPolygon(boards));
    // drop slivers left where a board edge and the outline run on the same line
    geo_of(geo::MultiPolygon(rest.0.into_iter().filter(|q| q.unsigned_area() > 1e-4).collect()))
}

fn geo_of(mp: geo::MultiPolygon<f64>) -> OutlineGeo {
    use geo::orient::{Direction, Orient};
    use geo::TriangulateEarcut;
    // exterior rings counter-clockwise, holes clockwise: the outward side is then the same for all edges
    let mp = mp.orient(Direction::Default);
    let mut g = OutlineGeo { rings: Vec::new(), verts: Vec::new(), tris: Vec::new(), walls: Vec::new() };
    for poly in mp.0.iter() {
        for ls in std::iter::once(poly.exterior()).chain(poly.interiors().iter()) {
            let mut r: Vec<Pt> = ls.0.iter().map(|c| (c.x, c.y)).collect();
            r.pop();
            if r.len() >= 3 {
                g.walls.extend((0..r.len()).map(|i| (r[i], r[(i + 1) % r.len()])));
                g.rings.push(r);
            }
        }
        let raw = poly.earcut_triangles_raw();
        let base = g.verts.len() as u32;
        g.verts.extend(raw.vertices.iter().map(|v| (v[0], v[1])));
        g.tris.extend(raw.triangle_indices.chunks(3).map(|t| [base + t[0] as u32, base + t[1] as u32, base + t[2] as u32]));
    }
    g
}

/// set or replace `(name x y)` inside the setup node
fn set_setup_point(setup: &mut Sexp, name: &str, p: Pt) {
    if let Some(v) = setup.children_mut() {
        v.retain(|c| c.name() != Some(name));
        v.push(node(name, xy(p)));
    }
}

// ------------------------------------------------------------- main entry

pub struct ExportStats {
    pub boards: usize,
    pub items_per_board: usize,
    pub outline_rings: usize,
    pub holes: usize,
}

pub fn export(
    source: &str,
    p: &Panel,
    cfg: &PanelConfig,
    sep: &Sep,
    feats: &Features,
) -> Result<(String, ExportStats), String> {
    let root = crate::sexpr::parse(source)?;
    if root.name() != Some("kicad_pcb") {
        return Err("source is not a kicad_pcb".into());
    }
    STYLE.with(|s| {
        s.set(Style {
            hide_list: source.contains("(hide yes)") || !source.contains(" hide)") && !source.contains(" hide "),
            fill_none: source.contains("(fill none)"),
        })
    });
    let origin = cfg.origin;
    let off = |q: Pt| (q.0 + origin.0, q.1 + origin.1);

    // split source
    let mut header: Vec<Sexp> = Vec::new();
    let mut nets: Vec<(i64, String, bool)> = Vec::new(); // id, name, numbered
    let mut items: Vec<&Sexp> = Vec::new();
    for c in root.children().iter().skip(1) {
        match c.name() {
            Some(n) if HEADER.contains(&n) => header.push(c.clone()),
            Some("net") => {
                if let Some(id) = c.atom(1).and_then(|s| s.parse::<i64>().ok()).filter(|_| !c.is_quoted(1)) {
                    nets.push((id, c.atom(2).unwrap_or("").to_string(), true));
                } else if let Some(name) = c.atom(1) {
                    nets.push((0, name.to_string(), false));
                }
            }
            Some(n) if DROP.contains(&n) => {}
            Some(n) if n.starts_with("gr_") && on_layer(c, "Edge.Cuts") => {}
            Some(_) => items.push(c),
            None => {}
        }
    }
    let stride = nets.iter().map(|n| n.0).max().unwrap_or(0) + 1;
    let nm = NetMap { names: nets.iter().map(|n| (n.0, n.1.clone())).collect(), stride, tpl: &cfg.net_template };
    let (pw, ph) = p.size;

    // page size that fits the panel with the same margin on all sides
    if cfg.page_fit {
        let paper = node("paper", vec![Sexp::str("User"), Sexp::num(pw + 2.0 * origin.0.max(5.0)), Sexp::num(ph + 2.0 * origin.1.max(5.0))]);
        match header.iter_mut().find(|h| h.name() == Some("paper")) {
            Some(h) => *h = paper,
            None => header.push(paper),
        }
    }
    // drill / place origin and grid origin on a panel corner
    if cfg.aux_origin != OriginCorner::None {
        let pt = off(if cfg.aux_origin == OriginCorner::TopLeft { (0.0, 0.0) } else { (0.0, ph) });
        if !header.iter().any(|h| h.name() == Some("setup")) {
            header.push(node("setup", vec![]));
        }
        if let Some(setup) = header.iter_mut().find(|h| h.name() == Some("setup")) {
            set_setup_point(setup, "aux_axis_origin", pt);
            set_setup_point(setup, "grid_origin", pt);
        }
    }

    let mut out: Vec<Sexp> = vec![Sexp::sym("kicad_pcb")];
    out.extend(header);

    // nets: keep "" net once, then per copy
    let numbered = nets.iter().any(|n| n.2);
    if numbered {
        out.push(node("net", vec![Sexp::sym("0"), Sexp::str("")]));
    }
    for copy in 0..p.boards.len() {
        for (id, name, num) in &nets {
            if *num && *id == 0 {
                continue;
            }
            if name.is_empty() && !num {
                continue;
            }
            let renamed = Sexp::Atom(rename_net(name, copy, &cfg.net_template), true);
            if *num {
                out.push(node("net", vec![Sexp::sym(&(id + copy as i64 * stride).to_string()), renamed]));
            } else {
                out.push(node("net", vec![renamed]));
            }
        }
    }

    // board copies
    for copy in 0..p.boards.len() {
        let xf = Xform { panel: p, idx: copy, origin };
        for it in &items {
            let mut n = (*it).clone();
            if n.name() == Some("footprint") {
                transform_footprint(&mut n, &xf);
                if cfg.ref_template != "{orig}" {
                    rename_ref(&mut n, copy, &cfg.ref_template);
                }
            } else {
                transform_abs(&mut n, &xf);
            }
            renew_uuids(&mut n);
            remap_nets(&mut n, copy, &nm);
            out.push(n);
        }
    }

    // outline
    let mp = panel_outline(p, sep, cfg);
    let mut rings = 0;
    for poly in mp.0.iter() {
        for ls in std::iter::once(poly.exterior()).chain(poly.interiors().iter()) {
            let mut r: Vec<Pt> = ls.0.iter().map(|c| off((c.x, c.y))).collect();
            r.pop(); // closing point
            if r.len() >= 3 {
                out.push(gr_poly(&r, "Edge.Cuts", cfg.edge_width.max(0.01), false));
                rings += 1;
            }
        }
    }

    // V-cuts
    let ext = cfg.vcut_extend.max(0.0);
    for v in &sep.vcuts {
        let (a, b) = if v.vertical { ((v.pos, -ext), (v.pos, ph + ext)) } else { ((-ext, v.pos), (pw + ext, v.pos)) };
        out.push(gr_line(off(a), off(b), &cfg.vcut_layer, 0.1));
    }
    if !sep.vcuts.is_empty() {
        out.push(gr_text("V-CUT", off((pw / 2.0, -1.5 - ext)), 1.0, &cfg.vcut_layer));
    }

    // mousebites
    let mut k = 1;
    for &(c, d) in &sep.holes {
        out.push(footprint("MouseBite", &format!("KMB{k}"), "MouseBite", false, off(c), npth_pad(d)));
        k += 1;
    }
    // tooling + fiducials
    let mut t = 1;
    let mut f = 1;
    for Feature { kind, pos, .. } in &feats.items {
        match kind {
            FeatureKind::Tooling => {
                let pad = if cfg.tool_pth { pth_pad(cfg.tool_d) } else { npth_pad(cfg.tool_d) };
                out.push(footprint("Tooling", &format!("KT{t}"), "Tooling", false, off(*pos), pad));
                t += 1;
            }
            FeatureKind::FiducialTop | FeatureKind::FiducialBottom => {
                let back = *kind == FeatureKind::FiducialBottom;
                out.push(footprint(
                    "Fiducial",
                    &format!("KF{f}"),
                    "Fiducial",
                    back,
                    off(*pos),
                    fid_pad(cfg.fid_copper, cfg.fid_opening, back),
                ));
                f += 1;
            }
        }
    }
    for txt in &feats.texts {
        out.push(panel_text(txt, off(txt.pos)));
    }
    // copper fill on the rails, both sides
    for r in &feats.copper {
        let ring = [off(r.min), off((r.max.0, r.min.1)), off(r.max), off((r.min.0, r.max.1))];
        for layer in ["F.Cu", "B.Cu"] {
            out.push(gr_poly(&ring, layer, 0.0, true));
        }
    }

    let mut s = String::new();
    Sexp::list(out).write(&mut s);
    let _ = fmt_num;
    Ok((
        s,
        ExportStats { boards: p.boards.len(), items_per_board: items.len(), outline_rings: rings, holes: sep.holes.len() },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::{self, PanelConfig, Separation};

    fn demo() -> (String, crate::board::Outline) {
        let src = std::fs::read_to_string("examples/demo.kicad_pcb").unwrap();
        let root = crate::sexpr::parse(&src).unwrap();
        let o = crate::board::extract_outline(&root).unwrap();
        (src, o)
    }

    /// settings that switch on most of the optional features at once
    fn full_cfg() -> PanelConfig {
        use crate::panel::*;
        PanelConfig {
            rows: 2,
            cols: 3,
            hspace: 7.0,
            vspace: 3.0,
            hspace_alt: 1.0,
            alternation: Alternation::Cols,
            bone_cols: 2.5,
            framing: Framing::Frame,
            corner: CornerStyle::Fillet,
            corner_size: 3.0,
            copper_fill: true,
            mill_radius: 1.0,
            mb_prolong: 0.8,
            tab_mode: TabMode::Spacing,
            tab_spacing: 20.0,
            tab_width: 3.0,
            tab_width_lr: 4.0,
            tool_edge: 2.0,
            texts: vec![
                TextCfg { text: "{board} {date} {cols}x{rows}".into(), rail: TextRail::Bottom, ..Default::default() },
                TextCfg { text: "BACK".into(), rail: TextRail::Top, layer: "B.SilkS".into(), justify: Justify::End, offset: 20.0, ..Default::default() },
                TextCfg { text: "SIDE".into(), rail: TextRail::Left, justify: Justify::Start, offset: 12.0, ..Default::default() },
            ],
            net_template: "P{n}/{orig}".into(),
            ref_template: "{orig}_{n}".into(),
            edge_width: 0.1,
            page_fit: true,
            aux_origin: OriginCorner::BottomLeft,
            ..Default::default()
        }
    }

    #[test]
    fn export_with_all_options() {
        let (src, o) = demo();
        let cfg = full_cfg();
        let p = panel::build(&o, &cfg);
        let sep = panel::separation(&p, &cfg);
        let f = panel::features(&p, &cfg, &sep.vcuts, "demo");
        let (text, st) = export(&src, &p, &cfg, &sep, &f).unwrap();
        assert_eq!(st.boards, 6);
        // one outline, everything connected: 1 outer ring (+ cutouts)
        let mp = panel_outline(&p, &sep, &cfg);
        assert_eq!(mp.0.len(), 1, "panel falls apart into {} pieces", mp.0.len());

        let re = crate::sexpr::parse(&text).unwrap();
        assert_eq!(re.name(), Some("kicad_pcb"));
        // page fitted, origin on the bottom-left panel corner
        let paper = re.child("paper").unwrap();
        assert_eq!(paper.atom(1), Some("User"));
        assert!((paper.f64_at(2).unwrap() - (p.size.0 + 2.0 * cfg.origin.0)).abs() < 1e-6);
        let aux = re.child("setup").unwrap().xy("aux_axis_origin").unwrap();
        assert!((aux.0 - cfg.origin.0).abs() < 1e-6 && (aux.1 - (cfg.origin.1 + p.size.1)).abs() < 1e-6);
        // texts: placeholders filled, back text mirrored, side text turned
        let texts: Vec<&Sexp> = re.children_named("gr_text").collect();
        assert!(texts.iter().any(|t| t.atom(1).map(|s| s.starts_with("demo 20") && s.ends_with("3x2")).unwrap_or(false)));
        let back = texts.iter().find(|t| t.atom(1) == Some("BACK")).unwrap();
        let just = back.child("effects").unwrap().child("justify").unwrap();
        assert!(just.children().iter().any(|a| matches!(a, Sexp::Atom(s, _) if s == "mirror")));
        let side = texts.iter().find(|t| t.atom(1) == Some("SIDE")).unwrap();
        assert_eq!(side.child("at").unwrap().f64_at(3), Some(90.0));
        // copper on both sides of the rails
        let cu = |l: &str| re.children_named("gr_poly").filter(|g| on_layer(g, l)).count();
        assert!(cu("F.Cu") > 0 && cu("F.Cu") == cu("B.Cu"));
        // outline width
        let edge = re.children_named("gr_poly").find(|g| on_layer(g, "Edge.Cuts")).unwrap();
        assert_eq!(edge.child("stroke").unwrap().child("width").unwrap().f64_at(1), Some(0.1));
        // name patterns
        if src.contains("(net ") {
            assert!(text.contains("\"P1/") && text.contains("\"P6/"));
            assert!(!text.contains("Board_1-"));
        }

        if let Ok(dir) = std::env::var("KIPANEL_OUT") {
            std::fs::write(std::path::Path::new(&dir).join("full.kicad_pcb"), &text).unwrap();
            for (name, cfg) in [
                ("default", PanelConfig::default()),
                ("vcut", PanelConfig { cols: 3, hspace: 0.0, vspace: 0.0, rail_space: 0.0, sep: Separation::VCut, vcut_extend: 3.0, corner: crate::panel::CornerStyle::Chamfer, ..Default::default() }),
            ] {
                let p = panel::build(&o, &cfg);
                let sep = panel::separation(&p, &cfg);
                let f = panel::features(&p, &cfg, &sep.vcuts, "demo");
                let (text, _) = export(&src, &p, &cfg, &sep, &f).unwrap();
                std::fs::write(std::path::Path::new(&dir).join(format!("{name}.kicad_pcb")), &text).unwrap();
            }
        }
    }

    #[test]
    fn frame_is_the_panel_without_the_boards() {
        let (_, o) = demo();
        let ring = |r: &[Pt]| (0..r.len()).map(|i| { let (a, b) = (r[i], r[(i + 1) % r.len()]); a.0 * b.1 - b.0 * a.1 }).sum::<f64>().abs() / 2.0;
        let tri_area = |g: &OutlineGeo| g.tris.iter().map(|t| ring(&[g.verts[t[0] as usize], g.verts[t[1] as usize], g.verts[t[2] as usize]])).sum::<f64>();
        for cfg in [PanelConfig::default(), PanelConfig { framing: crate::panel::Framing::Frame, bone_cols: 3.0, hspace: 8.0, ..Default::default() }] {
            let p = panel::build(&o, &cfg);
            let sep = panel::separation(&p, &cfg);
            let (all, frame) = (tri_area(&outline_geo(&p, &sep, &cfg)), tri_area(&frame_geo(&p, &sep, &cfg)));
            // inside a board the panel is the board: outer loop minus its cutouts
            let boards: f64 = p.boards.iter().map(|b| ring(&b.loops[0]) - b.loops[1..].iter().map(|l| ring(l)).sum::<f64>()).sum();
            assert!(frame > 1.0, "no frame left");
            assert!((all - boards - frame).abs() < 0.01 * frame, "{all} - {boards} vs {frame}");
        }
    }

    #[test]
    fn corner_shapes_and_mill_radius() {
        use crate::panel::CornerStyle;
        use geo::Area;
        let (_, o) = demo();
        let base = PanelConfig { framing: crate::panel::Framing::Frame, ..Default::default() };
        let area = |cfg: &PanelConfig| {
            let p = panel::build(&o, cfg);
            let sep = panel::separation(&p, cfg);
            panel_outline(&p, &sep, cfg).unsigned_area()
        };
        let square = area(&base);
        // chamfer removes 4 triangles, a round corner 4 × (r² − πr²/4)
        let chamfer = area(&PanelConfig { corner: CornerStyle::Chamfer, corner_size: 3.0, ..base.clone() });
        assert!((square - chamfer - 4.0 * 4.5).abs() < 1e-6);
        let round = area(&PanelConfig { corner: CornerStyle::Fillet, corner_size: 3.0, ..base.clone() });
        let cut = 4.0 * (9.0 - std::f64::consts::PI * 9.0 / 4.0);
        // the arc is a 12-segment polygon, which cuts slightly more than a true circle
        assert!((square - round - cut).abs() < 0.1, "{} vs {}", square - round, cut);
        // rounding inner corners only adds material, and only a little — even when the radius is
        // half the gap (2 mm gaps, 1 mm radius), where a naive rounding would close the slots
        let milled = area(&PanelConfig { mill_radius: 1.0, ..base.clone() });
        assert!(milled > square && milled < square * 1.02, "{square} -> {milled}");
        let big = area(&PanelConfig { mill_radius: 3.0, ..base.clone() });
        assert!(big < square * 1.02, "{square} -> {big}");
    }

    #[test]
    #[ignore]
    fn export_sample() {
        let path = std::env::var("KIPANEL_SAMPLE").expect("set KIPANEL_SAMPLE");
        let src = std::fs::read_to_string(&path).unwrap();
        let root = crate::sexpr::parse(&src).unwrap();
        let o = crate::board::extract_outline(&root).unwrap();
        for (name, cfg) in [
            ("mb", PanelConfig { rows: 2, cols: 2, rotation: 90, texts: vec![crate::panel::TextCfg { text: "JLCJLCJLCJLC".into(), ..Default::default() }], ref_template: "{orig}_{n}".into(), ..Default::default() }),
            ("vc", PanelConfig { rows: 1, cols: 3, hspace: 0.0, vspace: 0.0, rail_space: 0.0, sep: Separation::VCut, ..Default::default() }),
        ] {
            let p = panel::build(&o, &cfg);
            let sep = panel::separation(&p, &cfg);
            let f = panel::features(&p, &cfg, &sep.vcuts, "board");
            let mp = panel_outline(&p, &sep, &cfg);
            eprintln!("rails {} tabs {} fills {} -> polys {} interiors {:?} loops/board {}", p.rails.len(), sep.tabs.len(), sep.fills.len(), mp.0.len(), mp.0.iter().map(|q| q.interiors().len()).collect::<Vec<_>>(), p.boards[0].loops.len());
            let (text, st) = export(&src, &p, &cfg, &sep, &f).unwrap();
            let re = crate::sexpr::parse(&text).unwrap();
            eprintln!("{name}: boards {} items {} rings {} holes {} warnings {:?} {:?}", st.boards, st.items_per_board, st.outline_rings, st.holes, sep.warnings, f.warnings);
            assert_eq!(re.name(), Some("kicad_pcb"));
            std::fs::write(format!("/tmp/out_{name}.kicad_pcb"), &text).unwrap();
        }
    }
    #[test]
    fn tabs_stay_off_rounded_corners() {
        // 20 x 30 mm board with 1.25 mm corner radii in 2 degree steps, like KiCad arcs are read
        let (w, h, r) = (20.0, 30.0, 1.25);
        let mut l: Vec<Pt> = Vec::new();
        for (k, c) in [(w - r, r), (w - r, h - r), (r, h - r), (r, r)].into_iter().enumerate() {
            for i in 0..=45 {
                let a = (k as f64 * 90.0 - 90.0 + i as f64 * 2.0).to_radians();
                l.push((c.0 + r * a.cos(), c.1 + r * a.sin()));
            }
        }
        let o = crate::board::Outline { loops: vec![l], open: vec![], min: (0.0, 0.0), max: (w, h), separate: 0 };
        // top / bottom tabs a hair wider than the straight part of the edge: they would only fit
        // if the first arc segments counted as straight, and must therefore not be placed at all
        let cfg = PanelConfig { rows: 2, cols: 2, tab_width: w - 2.0 * r + 0.08, tabs_h: 1, tabs_v: 1, ..Default::default() };
        let p = panel::build(&o, &cfg);
        let sep = panel::separation(&p, &cfg);
        assert!(!sep.tabs.is_empty());
        for ring in outline_geo(&p, &sep, &cfg).rings {
            let n = ring.len();
            for i in 0..n {
                let (a, b, c) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
                let (u, v) = ((b.0 - a.0, b.1 - a.1), (c.0 - b.0, c.1 - b.1));
                let cos = (u.0 * v.0 + u.1 * v.1) / (u.0.hypot(u.1) * v.0.hypot(v.1));
                assert!(cos > -0.9, "outline doubles back at {b:?}");
            }
        }
    }
}
