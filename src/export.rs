//! Write the panel as a new .kicad_pcb file.
//!
//! - every board copy is cloned from the source S-expression, transformed (rotation + shift),
//!   gets fresh UUIDs and per-copy net names (Board_N-<net>)
//! - original Edge.Cuts are removed; the panel outline is the union of boards, rails, tabs, fills
//! - V-cut lines, mousebite / tooling NPTH holes, fiducials and text are added

use crate::panel::{Feature, FeatureKind, Features, Panel, PanelConfig, Sep};
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
        self.panel.rotation as f64
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

struct NetMap {
    /// old net number -> name
    names: HashMap<i64, String>,
    stride: i64,
}

fn rename_net(name: &str, copy: usize) -> String {
    if name.is_empty() { String::new() } else { format!("Board_{}-{}", copy + 1, name) }
}

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
                    n.set(2, Sexp::Atom(rename_net(&old, copy), true));
                }
            } else if n.is_quoted(1) {
                let old = n.atom(1).unwrap_or("").to_string();
                n.set(1, Sexp::Atom(rename_net(&old, copy), true));
            }
            return;
        }
        Some("net_name") if n.is_quoted(1) => {
            let old = n.atom(1).unwrap_or("").to_string();
            n.set(1, Sexp::Atom(rename_net(&old, copy), true));
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

fn rename_ref(fp: &mut Sexp, copy: usize) {
    if let Some(v) = fp.children_mut() {
        for c in v.iter_mut() {
            let is_ref = (c.name() == Some("property") && c.atom(1) == Some("Reference"))
                || (c.name() == Some("fp_text") && c.atom(1) == Some("reference"));
            if is_ref {
                let old = c.atom(2).unwrap_or("").to_string();
                c.set(2, Sexp::Atom(format!("{old}_{}", copy + 1), true));
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

fn gr_poly(ring: &[Pt], layer: &str) -> Sexp {
    let pts: Vec<Sexp> = ring.iter().map(|&p| node("xy", xy(p))).collect();
    node(
        "gr_poly",
        vec![
            node("pts", pts),
            node("stroke", vec![node("width", vec![Sexp::num(0.05)]), node("type", vec![Sexp::sym("solid")])]),
            node("fill", vec![Sexp::sym(if style().fill_none { "none" } else { "no" })]),
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

pub fn panel_outline(p: &Panel, sep: &Sep) -> MultiPolygon<f64> {
    let mut polys: Vec<Polygon<f64>> = Vec::new();
    for b in &p.boards {
        if let Some(outer) = b.loops.first() {
            let holes = b.loops.iter().skip(1).map(|h| ring_to_ls(h)).collect();
            polys.push(Polygon::new(ring_to_ls(outer), holes));
        }
    }
    for r in p.rails.iter().chain(sep.tabs.iter()).chain(sep.fills.iter()) {
        if r.w() > 1e-6 && r.h() > 1e-6 {
            polys.push(rect_poly(r));
        }
    }
    // consistent winding (exterior CCW, holes CW) — mixed winding breaks the union
    use geo::orient::{Direction, Orient};
    let polys: Vec<Polygon<f64>> = polys.into_iter().map(|p| p.orient(Direction::Default)).collect();
    unary_union(polys.iter())
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
    let nm = NetMap { names: nets.iter().map(|n| (n.0, n.1.clone())).collect(), stride };

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
            let renamed = Sexp::Atom(rename_net(name, copy), true);
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
                if cfg.rename_refs {
                    rename_ref(&mut n, copy);
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
    let mp = panel_outline(p, sep);
    let mut rings = 0;
    for poly in mp.0.iter() {
        for ls in std::iter::once(poly.exterior()).chain(poly.interiors().iter()) {
            let mut r: Vec<Pt> = ls.0.iter().map(|c| off((c.x, c.y))).collect();
            r.pop(); // closing point
            if r.len() >= 3 {
                out.push(gr_poly(&r, "Edge.Cuts"));
                rings += 1;
            }
        }
    }

    // V-cuts
    let (pw, ph) = p.size;
    for v in &sep.vcuts {
        let (a, b) = if v.vertical { ((v.pos, 0.0), (v.pos, ph)) } else { ((0.0, v.pos), (pw, v.pos)) };
        out.push(gr_line(off(a), off(b), &cfg.vcut_layer, 0.1));
    }
    if !sep.vcuts.is_empty() {
        out.push(gr_text("V-CUT", off((pw / 2.0, -1.5)), 1.0, &cfg.vcut_layer));
    }

    // mousebites
    let mut k = 1;
    for &(c, d) in &sep.holes {
        out.push(footprint("kipanel:MouseBite", &format!("KMB{k}"), "MouseBite", false, off(c), npth_pad(d)));
        k += 1;
    }
    // tooling + fiducials
    let mut t = 1;
    let mut f = 1;
    for Feature { kind, pos, .. } in &feats.items {
        match kind {
            FeatureKind::Tooling => {
                let pad = if cfg.tool_pth { pth_pad(cfg.tool_d) } else { npth_pad(cfg.tool_d) };
                out.push(footprint("kipanel:Tooling", &format!("KT{t}"), "Tooling", false, off(*pos), pad));
                t += 1;
            }
            FeatureKind::FiducialTop | FeatureKind::FiducialBottom => {
                let back = *kind == FeatureKind::FiducialBottom;
                out.push(footprint(
                    "kipanel:Fiducial",
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
    if let Some(txt) = &feats.text {
        out.push(gr_text(&txt.text, off(txt.pos), txt.size, "F.SilkS"));
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

    #[test]
    #[ignore]
    fn export_sample() {
        let path = std::env::var("KIPANEL_SAMPLE").expect("set KIPANEL_SAMPLE");
        let src = std::fs::read_to_string(&path).unwrap();
        let root = crate::sexpr::parse(&src).unwrap();
        let o = crate::board::extract_outline(&root).unwrap();
        for (name, cfg) in [
            ("mb", PanelConfig { rows: 2, cols: 2, rotation: 90, text: "JLCJLCJLCJLC".into(), rename_refs: true, ..Default::default() }),
            ("vc", PanelConfig { rows: 1, cols: 3, hspace: 0.0, vspace: 0.0, rail_space: 0.0, sep: Separation::VCut, ..Default::default() }),
        ] {
            let p = panel::build(&o, &cfg);
            let sep = panel::separation(&p, &cfg);
            let f = panel::features(&p, &cfg, &sep.vcuts);
            let mp = panel_outline(&p, &sep);
            eprintln!("rails {} tabs {} fills {} -> polys {} interiors {:?} loops/board {}", p.rails.len(), sep.tabs.len(), sep.fills.len(), mp.0.len(), mp.0.iter().map(|q| q.interiors().len()).collect::<Vec<_>>(), p.boards[0].loops.len());
            let (text, st) = export(&src, &p, &cfg, &sep, &f).unwrap();
            let re = crate::sexpr::parse(&text).unwrap();
            eprintln!("{name}: boards {} items {} rings {} holes {} warnings {:?} {:?}", st.boards, st.items_per_board, st.outline_rings, st.holes, sep.warnings, f.warnings);
            assert_eq!(re.name(), Some("kicad_pcb"));
            std::fs::write(format!("/tmp/out_{name}.kicad_pcb"), &text).unwrap();
        }
    }
}
