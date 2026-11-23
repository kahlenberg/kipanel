//! "Fix automatically": recognise the problems the checks report and adjust the settings so they go away.
//!
//! Every rule looks at the same facts as the check it belongs to (not at the message text), changes
//! as little as possible, and records what it changed. Rules are applied in passes, because one fix
//! can make another one possible or necessary (e.g. wider rails first, then holes can be placed).

use crate::board::Outline;
use crate::panel::{
    build, features, separation, Corners, CornerStyle, Features, Framing, Justify, Panel, PanelConfig, Separation, TabMode, TextCfg, TextRail, VCutLine,
};

const TOL: f64 = 1e-3;
const MAX_PASSES: usize = 8;

/// what was changed: (setting, before, after)
#[derive(Default)]
struct Log(Vec<(String, String, String)>);

impl Log {
    fn add(&mut self, label: &str, from: String, to: String) {
        match self.0.iter_mut().find(|e| e.0 == label) {
            Some(e) => e.2 = to, // changed again in a later pass: keep the first "before"
            None => self.0.push((label.to_string(), from, to)),
        }
    }
    fn mm(&mut self, label: &str, field: &mut f64, new: f64) {
        if (*field - new).abs() > 1e-9 {
            self.add(label, fmt_mm(*field), fmt_mm(new));
            *field = new;
        }
    }
    fn lines(self) -> Vec<String> {
        self.0.into_iter().filter(|(_, a, b)| a != b).map(|(l, a, b)| format!("{l}: {a} -> {b}")).collect()
    }
}

fn fmt_mm(v: f64) -> String {
    let s = format!("{v:.2}");
    format!("{} mm", s.trim_end_matches('0').trim_end_matches('.'))
}

/// round up to a multiple of `step`
fn ceil_to(v: f64, step: f64) -> f64 {
    ((v - 1e-9) / step).ceil() * step
}

fn state(o: &Outline, c: &PanelConfig, board: &str) -> (Panel, crate::panel::Sep, Features) {
    let p = build(o, c);
    let s = separation(&p, c);
    let f = features(&p, c, &s.vcuts, board);
    (p, s, f)
}

/// Returns the adjusted settings and a list of the changes ("Column spacing: 0 mm -> 2 mm").
pub fn optimize(o: &Outline, cfg: &PanelConfig, board: &str) -> (PanelConfig, Vec<String>) {
    let mut c = cfg.clone();
    let mut log = Log::default();
    for _ in 0..MAX_PASSES {
        let before = serde_json::to_string(&c).unwrap_or_default();
        let (p, s, f) = state(o, &c, board);
        general(&mut c, &p, &mut log);
        match c.sep {
            Separation::VCut => vcut(&mut c, &p, &mut log),
            _ => tabs(&mut c, &p, &s, &mut log),
        }
        // the rules below place things on the panel as it is now; if the panel itself just
        // changed, do that in the next pass
        if serde_json::to_string(&c).unwrap_or_default() == before {
            holes_and_fiducials(&mut c, &p, &s.vcuts, &f, board, &mut log);
            let f = features(&p, &c, &s.vcuts, board);
            texts(&mut c, &p, &s.vcuts, &f, board, &mut log);
        }
        if serde_json::to_string(&c).unwrap_or_default() == before {
            break;
        }
    }
    (c, log.lines())
}

fn general(c: &mut PanelConfig, p: &Panel, log: &mut Log) {
    if !c.net_template.contains("{orig}") {
        let d = PanelConfig::default().net_template;
        log.add("Net names", c.net_template.clone(), d.clone());
        c.net_template = d;
    }
    if c.framing == Framing::None {
        // these sit on rails; without rails the panel is meant as bare boards
        if c.tooling != Corners::None {
            log.add("Tooling holes", c.tooling.label().into(), "None (no rails)".into());
            c.tooling = Corners::None;
        }
        if c.fiducials != Corners::None {
            log.add("Fiducials", c.fiducials.label().into(), "None (no rails)".into());
            c.fiducials = Corners::None;
        }
        if c.copper_fill {
            log.add("Copper fill", "on".into(), "off (no rails)".into());
            c.copper_fill = false;
        }
    } else if c.corner != CornerStyle::Square && c.framing != Framing::Frame {
        let limit = [p.rail_tb_w, p.rail_lr_w].into_iter().filter(|w| *w > TOL).fold(f64::MAX, f64::min);
        if c.corner_size > limit + TOL {
            log.mm("Panel corner size", &mut c.corner_size, limit);
        }
    }
    // a minimum above the maximum can never be met
    if c.max_w > TOL && c.min_w > c.max_w {
        log.mm("Min panel width", &mut c.min_w, c.max_w);
    }
    if c.max_h > TOL && c.min_h > c.max_h {
        log.mm("Min panel height", &mut c.min_h, c.max_h);
    }
}

fn vcut(c: &mut PanelConfig, p: &Panel, log: &mut Log) {
    // V-cuts need the boards edge to edge
    log.mm("Column spacing", &mut c.hspace, 0.0);
    log.mm("Row spacing", &mut c.vspace, 0.0);
    log.mm("Extra 2nd column gap", &mut c.hspace_alt, 0.0);
    log.mm("Extra 2nd row gap", &mut c.vspace_alt, 0.0);
    if c.framing != Framing::None {
        log.mm("Gap to boards", &mut c.rail_space, 0.0);
    }
    log.mm("Backbone in column gaps", &mut c.bone_cols, 0.0);
    log.mm("Backbone in row gaps", &mut c.bone_rows, 0.0);
    // many fabs only V-cut panels of at least 70 × 70 mm; rails can grow to reach that
    if c.framing != Framing::None {
        let fits = |max: f64| max < TOL || max >= 70.0;
        if p.size.0 < 70.0 - TOL && c.min_w < 70.0 && fits(c.max_w) {
            log.mm("Min panel width", &mut c.min_w, 70.0);
        }
        if p.size.1 < 70.0 - TOL && c.min_h < 70.0 && fits(c.max_h) {
            log.mm("Min panel height", &mut c.min_h, 70.0);
        }
    }
}

fn tabs(c: &mut PanelConfig, p: &Panel, s: &crate::panel::Sep, log: &mut Log) {
    // ---- gaps: the router must fit; with a backbone, on both sides of it
    let need = |bone: f64| if bone > TOL { bone + 2.0 * c.mill_d } else { c.mill_d };
    let (need_col, need_row) = (need(c.bone_cols), need(c.bone_rows));
    if p.cols > 1 {
        if c.hspace < need_col - TOL {
            log.mm("Column spacing", &mut c.hspace, ceil_to(need_col, 0.1));
        }
        if c.hspace + c.hspace_alt < need_col - TOL {
            log.mm("Extra 2nd column gap", &mut c.hspace_alt, 0.0);
        }
    }
    if p.rows > 1 {
        if c.vspace < need_row - TOL {
            log.mm("Row spacing", &mut c.vspace, ceil_to(need_row, 0.1));
        }
        if c.vspace + c.vspace_alt < need_row - TOL {
            log.mm("Extra 2nd row gap", &mut c.vspace_alt, 0.0);
        }
    }
    if c.framing != Framing::None && c.rail_space < c.mill_d - TOL {
        log.mm("Gap to boards", &mut c.rail_space, ceil_to(c.mill_d, 0.1));
    }

    // ---- mousebite holes
    if c.sep == Separation::Mousebites {
        if c.mb_drill < 0.5 - TOL {
            log.mm("Mousebite hole Ø", &mut c.mb_drill, 0.5);
        }
        if c.mb_spacing - c.mb_drill < 0.2 - TOL {
            log.mm("Mousebite hole pitch", &mut c.mb_spacing, ceil_to(c.mb_drill + 0.3, 0.1));
        }
    }

    // ---- tabs: must fit on the shortest straight edge they are asked to sit on
    let min_w = if c.sep == Separation::Mousebites { (c.mb_spacing * 2.0).max(1.0) } else { 1.0 };
    for (short, w, label) in [(s.short_h, &mut c.tab_width, "Tab width"), (s.short_v, &mut c.tab_width_lr, "Tab width left/right")] {
        if let Some(len) = short {
            // a little shorter than the edge, in 0.1 mm steps
            let new = ((len - 0.05) * 10.0).floor() / 10.0;
            if new >= min_w && new < *w {
                log.mm(label, w, new);
            }
        }
    }
    if c.tab_mode == TabMode::Count && c.tabs_h == 0 && c.tabs_v == 0 && (c.framing != Framing::None || p.boards.len() > 1) {
        log.add("Tabs top/bottom", "0".into(), "1".into());
        log.add("Tabs left/right", "0".into(), "1".into());
        c.tabs_h = 1;
        c.tabs_v = 1;
    }
}

/// rails wide enough, then a distance from the corner where nothing is in the way
fn holes_and_fiducials(c: &mut PanelConfig, p: &Panel, vcuts: &[VCutLine], f: &Features, board: &str, log: &mut Log) {
    if c.framing == Framing::None || (!f.tool_bad && !f.fid_bad) {
        return;
    }
    // 0.5 mm of rail on both sides of the hole / mask opening
    let mut need: f64 = 0.0;
    if c.tooling != Corners::None {
        need = need.max(c.tool_d + 1.0);
    }
    if c.fiducials != Corners::None {
        need = need.max(c.fid_opening.max(c.fid_copper) + 1.0);
    }
    if c.rail_width < need - TOL {
        log.mm("Rail width", &mut c.rail_width, ceil_to(need, 0.5));
        return; // the panel changes; place in the next pass
    }
    // corner features sit along the top/bottom rails if there are any, else along the side rails
    let along_len = if c.framing.has_tb() { p.size.0 } else { p.size.1 };
    let search = |c: &mut PanelConfig, tooling: bool, log: &mut Log| {
        let bad = |c: &PanelConfig| {
            let f = features(p, c, vcuts, board);
            if tooling { f.tool_bad } else { f.fid_bad }
        };
        if !bad(c) {
            return;
        }
        let (r, label_edge, label_along) = if tooling {
            (c.tool_d / 2.0, "Tooling holes: from edge", "Tooling holes: distance from corner")
        } else {
            (c.fid_opening.max(c.fid_copper) / 2.0, "Fiducials: from edge", "Fiducials: distance from corner")
        };
        let mut t = c.clone();
        // back to the middle of the rail first
        *(if tooling { &mut t.tool_edge } else { &mut t.fid_edge }) = 0.0;
        let old_along = if tooling { c.tool_along } else { c.fid_along };
        // nearest free distance to the current one
        let mut cands: Vec<f64> = Vec::new();
        let mut a = ceil_to(r + 1.0, 0.5);
        while a <= along_len / 2.0 - r {
            cands.push(a);
            a += 0.5;
        }
        cands.sort_by(|x, y| (x - old_along).abs().partial_cmp(&(y - old_along).abs()).unwrap());
        cands.insert(0, old_along);
        for a in cands {
            *(if tooling { &mut t.tool_along } else { &mut t.fid_along }) = a;
            if !bad(&t) {
                let (edge, along) = if tooling { (&mut c.tool_edge, &mut c.tool_along) } else { (&mut c.fid_edge, &mut c.fid_along) };
                if *edge > TOL {
                    log.add(label_edge, fmt_mm(*edge), "centered on rail".into());
                    *edge = 0.0;
                }
                log.mm(label_along, along, a);
                return;
            }
        }
    };
    // tooling holes first: fiducials then keep clear of them
    if c.tooling != Corners::None {
        search(c, true, log);
    }
    if c.fiducials != Corners::None {
        search(c, false, log);
    }
}

fn rail_exists(c: &PanelConfig, r: TextRail) -> bool {
    match r {
        TextRail::Top | TextRail::Bottom => c.framing.has_tb(),
        TextRail::Left | TextRail::Right => c.framing.has_lr(),
    }
}

/// find a rail, size and position where each problem text fits and covers nothing
fn texts(c: &mut PanelConfig, p: &Panel, vcuts: &[VCutLine], f: &Features, board: &str, log: &mut Log) {
    if c.framing == Framing::None {
        return; // nowhere to put them
    }
    // start / end positions begin behind the corner holes and fiducials
    let mut margin: f64 = 1.0;
    if c.tooling != Corners::None {
        margin = margin.max(c.tool_along + c.tool_d / 2.0 + 1.5);
    }
    if c.fiducials != Corners::None {
        margin = margin.max(c.fid_along + c.fid_opening.max(c.fid_copper) / 2.0 + 1.5);
    }
    let margin = ceil_to(margin, 0.5);
    for &i in &f.text_bad {
        let Some(old) = c.texts.get(i).cloned() else { continue };
        // keep the rail if possible, then the size, then the position
        let mut rails = vec![old.rail];
        rails.extend(TextRail::ALL.into_iter().filter(|r| *r != old.rail));
        let rail_w = |r: TextRail| if matches!(r, TextRail::Top | TextRail::Bottom) { p.rail_tb_w } else { p.rail_lr_w };
        let places = [(old.justify, old.offset), (Justify::Center, 0.0), (Justify::Start, margin), (Justify::End, margin)];
        let mut found: Option<TextCfg> = None;
        'search: for rail in rails.into_iter().filter(|r| rail_exists(c, *r)) {
            // not taller than the rail, then smaller in 0.1 mm steps down to 0.8 mm
            let mut size = old.size.min(((rail_w(rail) - 0.6) * 10.0).floor() / 10.0);
            while size >= 0.8 - 1e-9 {
                for (justify, offset) in places {
                    let cand = TextCfg { rail, size, justify, offset, ..old.clone() };
                    let mut t = c.clone();
                    t.texts[i] = cand.clone();
                    if !features(p, &t, vcuts, board).text_bad.contains(&i) {
                        found = Some(cand);
                        break 'search;
                    }
                }
                size = ((size - 0.1) * 10.0).round() / 10.0;
            }
        }
        if let Some(new) = found {
            let name = format!("Text {}", i + 1);
            if new.rail != old.rail {
                log.add(&format!("{name}: rail"), old.rail.label().into(), new.rail.label().into());
            }
            if (new.size - old.size).abs() > 1e-9 {
                log.add(&format!("{name}: height"), fmt_mm(old.size), fmt_mm(new.size));
            }
            if new.justify != old.justify || (new.offset - old.offset).abs() > 1e-9 {
                let show = |j: Justify, o: f64| match j {
                    Justify::Center if o.abs() < 1e-9 => "center".to_string(),
                    Justify::Center => format!("center, shifted {}", fmt_mm(o)),
                    Justify::Start => format!("start + {}", fmt_mm(o)),
                    Justify::End => format!("end − {}", fmt_mm(o)),
                };
                log.add(&format!("{name}: position"), show(old.justify, old.offset), show(new.justify, new.offset));
            }
            c.texts[i] = new;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f64, h: f64) -> Outline {
        Outline { loops: vec![vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]], open: vec![], min: (0.0, 0.0), max: (w, h), separate: 0 }
    }

    fn problems(o: &Outline, c: &PanelConfig) -> Vec<String> {
        let (_, s, f) = state(o, c, "demo");
        s.warnings.into_iter().chain(f.warnings).filter(|w| w.starts_with('⚠')).collect()
    }

    /// the fixer must remove every problem, and a second run must change nothing
    fn fixes(o: &Outline, c: &PanelConfig) -> (PanelConfig, Vec<String>) {
        assert!(!problems(o, c).is_empty(), "test setup has no problem");
        let (fixed, log) = optimize(o, c, "demo");
        assert!(problems(o, &fixed).is_empty(), "left over: {:?}\nchanges: {log:?}", problems(o, &fixed));
        assert!(!log.is_empty());
        assert!(optimize(o, &fixed, "demo").1.is_empty(), "not stable");
        (fixed, log)
    }

    #[test]
    fn nothing_to_do_on_a_good_panel() {
        let o = rect(40.0, 30.0);
        let c = PanelConfig::default();
        assert!(problems(&o, &c).is_empty());
        let (fixed, log) = optimize(&o, &c, "demo");
        assert!(log.is_empty(), "{log:?}");
        assert_eq!(serde_json::to_string(&fixed).unwrap(), serde_json::to_string(&c).unwrap());
    }

    #[test]
    fn gaps_too_small_for_the_router() {
        let o = rect(40.0, 30.0);
        let c = PanelConfig { rows: 2, hspace: 0.0, vspace: 1.0, rail_space: 0.5, ..Default::default() };
        let (f, log) = fixes(&o, &c);
        assert_eq!((f.hspace, f.vspace, f.rail_space), (2.0, 2.0, 2.0));
        assert!(log.iter().any(|l| l == "Column spacing: 0 mm -> 2 mm"), "{log:?}");
    }

    #[test]
    fn backbone_needs_a_wider_gap() {
        let o = rect(40.0, 30.0);
        let c = PanelConfig { hspace: 3.0, bone_cols: 3.0, ..Default::default() };
        let (f, _) = fixes(&o, &c);
        assert_eq!(f.hspace, 7.0); // bar + router on both sides
        assert_eq!(build(&o, &f).bones.len(), 1);
    }

    #[test]
    fn tab_too_wide_for_a_narrow_neck() {
        // top edge is only 4 mm long
        let o = Outline {
            loops: vec![vec![(3.0, 0.0), (7.0, 0.0), (7.0, 2.0), (30.0, 2.0), (30.0, 20.0), (0.0, 20.0), (0.0, 2.0), (3.0, 2.0)]],
            open: vec![],
            min: (0.0, 0.0),
            max: (30.0, 20.0),
            separate: 0,
        };
        let c = PanelConfig { cols: 1, tab_width: 6.0, tab_width_lr: 6.0, ..Default::default() };
        let (f, _) = fixes(&o, &c);
        assert_eq!(f.tab_width, 3.9);
        assert_eq!(f.tab_width_lr, 6.0); // side edges were fine
    }

    #[test]
    fn vcut_wants_zero_spacing_and_a_minimum_size() {
        let o = rect(20.0, 15.0);
        let c = PanelConfig { sep: Separation::VCut, ..Default::default() };
        let (f, log) = optimize(&o, &c, "demo");
        assert_eq!((f.hspace, f.rail_space), (0.0, 0.0));
        assert_eq!((f.min_w, f.min_h), (70.0, 70.0));
        let p = build(&o, &f);
        assert!(p.size.0 >= 70.0 - 1e-9 && p.size.1 >= 70.0 - 1e-9);
        let (_, s, ft) = state(&o, &f, "demo");
        assert!(s.warnings.is_empty() && ft.warnings.is_empty(), "{:?} {:?} {log:?}", s.warnings, ft.warnings);
    }

    #[test]
    fn rails_too_narrow_for_the_holes() {
        let o = rect(60.0, 30.0);
        let c = PanelConfig { rail_width: 2.0, tool_d: 3.0, ..Default::default() };
        let (f, _) = fixes(&o, &c);
        assert_eq!(f.rail_width, 4.0);
    }

    #[test]
    fn hole_on_a_vcut_and_fiducial_on_a_hole() {
        let o = rect(30.0, 40.0);
        // V-cut lines at x = 0, 30, 60, 90 on the rails; hole exactly on one, fiducial on the hole
        let c = PanelConfig { cols: 3, hspace: 0.0, vspace: 0.0, rail_space: 0.0, sep: Separation::VCut, tool_along: 30.0, fid_along: 30.5, ..Default::default() };
        let (f, log) = fixes(&o, &c);
        assert!((f.tool_along - 30.0).abs() >= 0.5, "{log:?}");
        assert!((f.fid_along - f.tool_along).abs() > 1.0);
    }

    #[test]
    fn no_rails_no_rail_features() {
        let o = rect(40.0, 30.0);
        let c = PanelConfig { framing: Framing::None, copper_fill: true, ..Default::default() };
        let (f, _) = fixes(&o, &c);
        assert_eq!((f.tooling, f.fiducials, f.copper_fill), (Corners::None, Corners::None, false));
    }

    #[test]
    fn text_is_resized_or_moved() {
        let o = rect(40.0, 30.0);
        // too long for the rail at this size
        let long = PanelConfig { cols: 1, texts: vec![TextCfg { text: "A".repeat(30), size: 3.0, ..Default::default() }], ..Default::default() };
        let (f, _) = fixes(&o, &long);
        assert!(f.texts[0].size < 3.0 && f.texts[0].size >= 0.8);
        // on a rail that does not exist: moved to one that does
        let side = PanelConfig { texts: vec![TextCfg { text: "REV A".into(), rail: TextRail::Left, ..Default::default() }], ..Default::default() };
        let (f, log) = fixes(&o, &side);
        assert_eq!(f.texts[0].rail, TextRail::Top);
        assert!(log.iter().any(|l| l.starts_with("Text 1: rail")), "{log:?}");
        // on top of a tooling hole: pushed clear of it
        let hit = PanelConfig { texts: vec![TextCfg { text: "REV A".into(), justify: Justify::Start, offset: 3.0, ..Default::default() }], ..Default::default() };
        let (f, _) = fixes(&o, &hit);
        assert_eq!(f.texts[0].rail, TextRail::Bottom);
    }

    #[test]
    fn several_problems_at_once() {
        let o = rect(25.0, 20.0);
        let c = PanelConfig {
            rows: 2,
            cols: 2,
            hspace: 0.5,
            vspace: 0.0,
            rail_width: 1.5,
            mb_drill: 0.6,
            mb_spacing: 0.7,
            tool_along: 1.0,
            texts: vec![TextCfg { text: "KIPANEL DEMO BOARD REV 1".into(), size: 2.5, ..Default::default() }],
            net_template: "X".into(),
            ..Default::default()
        };
        let (_, log) = fixes(&o, &c);
        assert!(log.len() >= 5, "{log:?}");
    }

    #[test]
    fn what_cannot_be_fixed_is_left_alone() {
        // panel wider than the allowed maximum: needs a decision (fewer boards), not a tweak
        let o = rect(40.0, 30.0);
        let c = PanelConfig { cols: 4, max_w: 100.0, ..Default::default() };
        let (f, log) = optimize(&o, &c, "demo");
        assert!(log.is_empty(), "{log:?}");
        assert_eq!(f.cols, 4);
        assert_eq!(problems(&o, &f).len(), 1);
    }
}
