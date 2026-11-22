//! Extract the Edge.Cuts outline from a .kicad_pcb S-expression and chain it into closed loops.

use crate::sexpr::Sexp;

pub type Pt = (f64, f64);

#[derive(Debug, Clone, Default)]
pub struct Outline {
    /// Closed loops (first point != last point). Index 0 = outer (largest area).
    pub loops: Vec<Vec<Pt>>,
    /// Segments that could not be closed into loops (outline error).
    pub open: Vec<Vec<Pt>>,
    pub min: Pt,
    pub max: Pt,
    /// number of additional outlines outside the main board (ignored)
    pub separate: usize,
}

impl Outline {
    pub fn width(&self) -> f64 {
        self.max.0 - self.min.0
    }
    pub fn height(&self) -> f64 {
        self.max.1 - self.min.1
    }
}

const EDGE: &str = "Edge.Cuts";
const TOL: f64 = 1e-3; // mm, endpoint matching tolerance
const ARC_SEG_DEG: f64 = 2.0;

fn on_edge(n: &Sexp) -> bool {
    n.child("layer").and_then(|l| l.atom(1)) == Some(EDGE)
}

fn arc_points(s: Pt, m: Pt, e: Pt) -> Vec<Pt> {
    // circle through 3 points
    let (ax, ay) = s;
    let (bx, by) = m;
    let (cx, cy) = e;
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-12 {
        return vec![s, e];
    }
    let ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d;
    let uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d;
    let r = ((ax - ux).powi(2) + (ay - uy).powi(2)).sqrt();
    let a0 = (ay - uy).atan2(ax - ux);
    let am = (by - uy).atan2(bx - ux);
    let a1 = (cy - uy).atan2(cx - ux);
    let tau = std::f64::consts::TAU;
    let norm = |a: f64| ((a % tau) + tau) % tau;
    // sweep direction: does going CCW from a0 pass am before a1?
    let ccw_to_end = norm(a1 - a0);
    let ccw_to_mid = norm(am - a0);
    let sweep = if ccw_to_mid < ccw_to_end { ccw_to_end } else { ccw_to_end - tau };
    let n = ((sweep.abs().to_degrees() / ARC_SEG_DEG).ceil() as usize).max(2);
    (0..=n)
        .map(|i| {
            let a = a0 + sweep * i as f64 / n as f64;
            (ux + r * a.cos(), uy + r * a.sin())
        })
        .collect()
}

fn circle_points(c: Pt, r: f64) -> Vec<Pt> {
    let n = (360.0 / ARC_SEG_DEG) as usize;
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        })
        .collect()
}

/// Collect Edge.Cuts primitives. Closed shapes go straight into `closed`, the rest into `segs`.
/// Footprint placement transform (KiCad: positive angle = counter-clockwise on screen, Y down).
#[derive(Clone, Copy)]
struct Xf {
    dx: f64,
    dy: f64,
    c: f64,
    s: f64,
}

impl Xf {
    const ID: Xf = Xf { dx: 0.0, dy: 0.0, c: 1.0, s: 0.0 };
    fn apply(&self, p: Pt) -> Pt {
        (self.dx + p.0 * self.c + p.1 * self.s, self.dy - p.0 * self.s + p.1 * self.c)
    }
}

fn add(p: Pt, xf: Xf) -> Pt {
    xf.apply(p)
}

fn collect(node: &Sexp, segs: &mut Vec<Vec<Pt>>, closed: &mut Vec<Vec<Pt>>, offset: Xf) {
    for n in node.children() {
        match n.name() {
            Some("gr_line") | Some("fp_line") if on_edge(n) => {
                if let (Some(s), Some(e)) = (n.xy("start"), n.xy("end")) {
                    segs.push(vec![add(s, offset), add(e, offset)]);
                }
            }
            Some("gr_arc") | Some("fp_arc") if on_edge(n) => {
                if let (Some(s), Some(m), Some(e)) = (n.xy("start"), n.xy("mid"), n.xy("end")) {
                    segs.push(arc_points(add(s, offset), add(m, offset), add(e, offset)));
                }
            }
            Some("gr_circle") | Some("fp_circle") if on_edge(n) => {
                if let (Some(c), Some(e)) = (n.xy("center"), n.xy("end")) {
                    let r = ((e.0 - c.0).powi(2) + (e.1 - c.1).powi(2)).sqrt();
                    closed.push(circle_points(add(c, offset), r));
                }
            }
            Some("gr_rect") | Some("fp_rect") if on_edge(n) => {
                if let (Some(s), Some(e)) = (n.xy("start"), n.xy("end")) {
                    let corners = [s, (e.0, s.1), e, (s.0, e.1)];
                    closed.push(corners.iter().map(|&p| add(p, offset)).collect());
                }
            }
            Some("gr_poly") | Some("fp_poly") if on_edge(n) => {
                if let Some(pts) = n.child("pts") {
                    let p: Vec<Pt> = pts
                        .children_named("xy")
                        .filter_map(|x| Some(add((x.f64_at(1)?, x.f64_at(2)?), offset)))
                        .collect();
                    if p.len() >= 3 {
                        closed.push(p);
                    }
                }
            }
            Some("footprint") => {
                // Edge.Cuts inside footprints (e.g. cutouts): local coords, rotated by footprint angle
                let at = n.child("at");
                let (x, y) = at
                    .and_then(|a| Some((a.f64_at(1)?, a.f64_at(2)?)))
                    .unwrap_or((0.0, 0.0));
                let ang = at.and_then(|a| a.f64_at(3)).unwrap_or(0.0).to_radians();
                let xf = Xf { dx: x, dy: y, c: ang.cos(), s: ang.sin() };
                collect(n, segs, closed, xf);
            }
            _ => {}
        }
    }
}

fn near(a: Pt, b: Pt) -> bool {
    (a.0 - b.0).abs() < TOL && (a.1 - b.1).abs() < TOL
}

/// Chain open polylines into closed loops by matching endpoints.
fn chain(mut segs: Vec<Vec<Pt>>) -> (Vec<Vec<Pt>>, Vec<Vec<Pt>>) {
    let mut loops = Vec::new();
    let mut open = Vec::new();
    while let Some(mut cur) = segs.pop() {
        loop {
            if cur.len() > 2 && near(cur[0], *cur.last().unwrap()) {
                cur.pop();
                loops.push(cur);
                break;
            }
            let end = *cur.last().unwrap();
            let found = segs.iter().position(|s| near(s[0], end) || near(*s.last().unwrap(), end));
            match found {
                Some(i) => {
                    let mut s = segs.swap_remove(i);
                    if !near(s[0], end) {
                        s.reverse();
                    }
                    cur.extend(s.into_iter().skip(1));
                }
                None => {
                    open.push(cur);
                    break;
                }
            }
        }
    }
    (loops, open)
}

fn area(p: &[Pt]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

pub fn extract_outline(root: &Sexp) -> Result<Outline, String> {
    let mut segs = Vec::new();
    let mut closed = Vec::new();
    collect(root, &mut segs, &mut closed, Xf::ID);
    let (mut loops, open) = chain(segs);
    loops.extend(closed);
    if loops.is_empty() && open.is_empty() {
        return Err("no Edge.Cuts outline found".into());
    }
    loops.sort_by(|a, b| area(b).partial_cmp(&area(a)).unwrap());

    // keep the largest loop + loops inside it (cutouts); separate outlines are dropped
    let mut separate = 0;
    if let Some(outer) = loops.first().cloned() {
        let mut kept = vec![outer.clone()];
        for l in loops.into_iter().skip(1) {
            if point_in(&outer, l[0]) {
                kept.push(l);
            } else {
                separate += 1;
            }
        }
        loops = kept;
    }

    let all = loops.iter().take(1).chain(open.iter()).flatten();
    let mut min = (f64::MAX, f64::MAX);
    let mut max = (f64::MIN, f64::MIN);
    for &(x, y) in all {
        min = (min.0.min(x), min.1.min(y));
        max = (max.0.max(x), max.1.max(y));
    }
    Ok(Outline { loops, open, min, max, separate })
}

fn point_in(poly: &[Pt], p: Pt) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}
