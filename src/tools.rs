//! Jobs that run `kicad-cli` on the exported panel file: design rule check, 3D render, 3D model export.
//! They can take a while, so the GUI runs them on a worker thread.

use std::path::{Path, PathBuf};
use std::process::Command;

/// camera position of a 3D render
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Top,
    Bottom,
    Angled,
}

impl View {
    pub const ALL: [View; 3] = [View::Top, View::Bottom, View::Angled];
    pub fn label(self) -> &'static str {
        match self {
            View::Top => "Top view",
            View::Bottom => "Bottom view",
            View::Angled => "Angled view",
        }
    }
    fn tag(self) -> &'static str {
        match self {
            View::Top => "top",
            View::Bottom => "bottom",
            View::Angled => "angled",
        }
    }
}

/// 3D model file formats `kicad-cli pcb export` can write
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    Step,
    Glb,
    Stl,
    Ply,
    Vrml,
    Brep,
    Xao,
}

impl Model {
    pub const ALL: [Model; 7] = [Model::Step, Model::Glb, Model::Stl, Model::Ply, Model::Vrml, Model::Brep, Model::Xao];
    pub fn label(self) -> &'static str {
        match self {
            Model::Step => "STEP (.step) — mechanical CAD",
            Model::Glb => "glTF binary (.glb) — web / Blender",
            Model::Stl => "STL (.stl) — 3D printing",
            Model::Ply => "PLY (.ply)",
            Model::Vrml => "VRML (.wrl)",
            Model::Brep => "BREP (.brep) — OpenCascade",
            Model::Xao => "XAO (.xao) — Salome",
        }
    }
    /// (kicad-cli sub command, file extension)
    fn cli(self) -> (&'static str, &'static str) {
        match self {
            Model::Step => ("step", "step"),
            Model::Glb => ("glb", "glb"),
            Model::Stl => ("stl", "stl"),
            Model::Ply => ("ply", "ply"),
            Model::Vrml => ("vrml", "wrl"),
            Model::Brep => ("brep", "brep"),
            Model::Xao => ("xao", "xao"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Job {
    Drc,
    Render { view: View, high: bool },
    Model(Model),
}

impl Job {
    pub fn label(&self) -> String {
        match self {
            Job::Drc => "Running design rule check…".into(),
            Job::Render { view, .. } => format!("Rendering 3D image ({})…", view.label().to_lowercase()),
            Job::Model(m) => format!("Exporting 3D model ({})…", m.cli().1),
        }
    }
}

pub enum Done {
    Drc(DrcReport),
    Render(PathBuf),
    Model(PathBuf),
}

#[derive(Clone, Debug)]
pub struct DrcItem {
    /// rule name, e.g. "clearance"; "unconnected" for missing connections
    pub kind: String,
    pub error: bool,
    pub text: String,
    /// affected objects
    pub what: Vec<String>,
    /// position in the panel file (mm)
    pub pos: Option<(f64, f64)>,
}

#[derive(Clone, Debug, Default)]
pub struct DrcReport {
    pub errors: usize,
    pub warnings: usize,
    pub unconnected: usize,
    pub items: Vec<DrcItem>,
    pub report: PathBuf,
}

/// Locate kicad-cli: next to pcbnew (path reported by KiCad), in the standard install folders, else on PATH.
pub fn find_cli(pcbnew: Option<&str>) -> PathBuf {
    let exe = if cfg!(windows) { "kicad-cli.exe" } else { "kicad-cli" };
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(p) = pcbnew.map(Path::new) {
        if let Some(dir) = p.parent() {
            cands.push(dir.join(exe));
        }
        // macOS: pcbnew sits in a nested app bundle, kicad-cli in the main one
        for a in p.ancestors() {
            if a.file_name().map(|n| n == "KiCad.app").unwrap_or(false) {
                cands.push(a.join("Contents/MacOS").join(exe));
            }
        }
    }
    if cfg!(target_os = "macos") {
        cands.push(PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"));
    }
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramW6432"] {
            let Some(pf) = std::env::var_os(var) else { continue };
            let Ok(rd) = std::fs::read_dir(Path::new(&pf).join("KiCad")) else { continue };
            let mut vers: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            vers.sort();
            cands.extend(vers.into_iter().rev().map(|v| v.join("bin").join(exe)));
        }
    }
    cands.into_iter().find(|c| c.is_file()).unwrap_or_else(|| PathBuf::from(exe))
}

fn run_cli(cli: &Path, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let mut cmd = Command::new(cli);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up
    }
    let out = cmd.output().map_err(|e| format!("could not start {}: {e}", cli.display()))?;
    if out.status.success() {
        return Ok(());
    }
    let msg = format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
    let tail: Vec<&str> = msg.lines().filter(|l| !l.trim().is_empty()).rev().take(3).collect();
    Err(format!("kicad-cli failed: {}", tail.into_iter().rev().collect::<Vec<_>>().join(" · ")))
}

/// Two 3D exports of kicad-cli running at the same time can lose one of the output files
/// (seen with KiCad 9.0.7), so they take turns.
static EXPORT_3D: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `<folder>/<panel stem><suffix>`
fn sibling(panel: &Path, suffix: &str) -> PathBuf {
    let stem = panel.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "panel".into());
    panel.with_file_name(format!("{stem}{suffix}"))
}

pub fn run(cli: &Path, panel: &Path, job: Job) -> Result<Done, String> {
    use std::ffi::OsStr;
    if !panel.is_file() {
        return Err(format!("{} not found — export the panel first", panel.display()));
    }
    let p = panel.as_os_str();
    match job {
        Job::Drc => {
            let report = sibling(panel, ".drc.json");
            let _ = std::fs::remove_file(&report);
            run_cli(
                cli,
                &["pcb", "drc", "--format", "json", "--units", "mm", "--severity-error", "--severity-warning", "-o"]
                    .into_iter()
                    .map(OsStr::new)
                    .chain([report.as_os_str(), p])
                    .collect::<Vec<_>>(),
            )?;
            let text = std::fs::read_to_string(&report).map_err(|e| format!("{}: {e}", report.display()))?;
            Ok(Done::Drc(parse_drc(&text, report)?))
        }
        Job::Render { view, high } => {
            let out = sibling(panel, &format!("_{}.png", view.tag()));
            let mut a: Vec<&OsStr> = ["pcb", "render", "-w", "1600", "-h", "1000", "--quality", if high { "high" } else { "basic" }]
                .into_iter()
                .map(OsStr::new)
                .collect();
            match view {
                View::Top => a.extend(["--side", "top"].map(OsStr::new)),
                View::Bottom => a.extend(["--side", "bottom"].map(OsStr::new)),
                // 315° instead of -45°: kicad-cli would read a leading minus as an option
                View::Angled => a.extend(["--side", "top", "--perspective", "--rotate", "315,0,30", "--zoom", "0.7"].map(OsStr::new)),
            }
            a.extend([OsStr::new("-o"), out.as_os_str(), p]);
            run_cli(cli, &a)?;
            if !out.is_file() {
                return Err("kicad-cli wrote no image".into());
            }
            Ok(Done::Render(out))
        }
        Job::Model(m) => {
            let (sub, ext) = m.cli();
            let out = sibling(panel, &format!(".{ext}"));
            let mut a: Vec<&OsStr> = ["pcb", "export", sub, "--force"].into_iter().map(OsStr::new).collect();
            if m != Model::Vrml {
                a.push(OsStr::new("--subst-models"));
            }
            a.extend([OsStr::new("-o"), out.as_os_str(), p]);
            let _turn = EXPORT_3D.lock().unwrap_or_else(|e| e.into_inner());
            let _ = std::fs::remove_file(&out);
            run_cli(cli, &a)?;
            if !out.is_file() {
                return Err("kicad-cli wrote no model file".into());
            }
            Ok(Done::Model(out))
        }
    }
}

/// Export the board itself (not the panel) as a GLB model with components, for the 3D view.
/// `board` is the board file text; it is written next to the project so that 3D models with
/// project-relative paths are found, and removed again afterwards.
pub fn board_glb(cli: &Path, board: &str, project_dir: Option<&Path>, stem: &str) -> Result<Vec<u8>, String> {
    use std::ffi::OsStr;
    let tmp = std::env::temp_dir().join("kipanel");
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let name = format!("_kipanel_3d_{}_{stem}", std::process::id());
    let file = format!("{name}.kicad_pcb");
    // the project folder may be read-only: fall back to the temp folder
    let input = project_dir
        .map(|d| d.join(&file))
        .filter(|f| std::fs::write(f, board).is_ok())
        .or_else(|| Some(tmp.join(&file)).filter(|f| std::fs::write(f, board).is_ok()))
        .ok_or("could not write a temporary board file")?;
    let out = tmp.join(format!("{name}.glb"));
    let args: Vec<&OsStr> = ["pcb", "export", "glb", "--force", "--subst-models", "--include-pads", "--include-tracks", "--include-zones", "--include-silkscreen", "--include-soldermask", "-o"]
        .into_iter()
        .map(OsStr::new)
        .chain([out.as_os_str(), input.as_os_str()])
        .collect();
    let export = || run_cli(cli, &args).and_then(|_| std::fs::read(&out).map_err(|_| "kicad-cli wrote no model file".to_string()));
    let turn = EXPORT_3D.lock().unwrap_or_else(|e| e.into_inner());
    // once more if it fails: another KiPanel window may have been exporting at the same moment
    let res = export().or_else(|_| {
        std::thread::sleep(std::time::Duration::from_secs(2));
        export()
    });
    drop(turn);
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&out);
    res
}

/// read the JSON report of `kicad-cli pcb drc --format json`
pub fn parse_drc(text: &str, report: PathBuf) -> Result<DrcReport, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("DRC report: {e}"))?;
    let mut r = DrcReport { report, ..Default::default() };
    for (key, unconnected) in [("violations", false), ("unconnected_items", true)] {
        for it in v.get(key).and_then(|x| x.as_array()).into_iter().flatten() {
            let s = |k: &str| it.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let error = s("severity") != "warning";
            let objs = it.get("items").and_then(|x| x.as_array());
            let what: Vec<String> = objs
                .into_iter()
                .flatten()
                .filter_map(|o| o.get("description").and_then(|d| d.as_str()).map(|d| d.to_string()))
                .collect();
            let pos = objs.into_iter().flatten().find_map(|o| {
                let p = o.get("pos")?;
                Some((p.get("x")?.as_f64()?, p.get("y")?.as_f64()?))
            });
            if unconnected {
                r.unconnected += 1;
            } else if error {
                r.errors += 1;
            } else {
                r.warnings += 1;
            }
            r.items.push(DrcItem { kind: if unconnected { "unconnected".into() } else { s("type") }, error, text: s("description"), what, pos });
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drc_json() {
        let j = r#"{"coordinate_units":"mm","unconnected_items":[
            {"description":"Missing connection between items","items":[{"description":"Pad 1","pos":{"x":3.0,"y":4.0}}],"severity":"error","type":"unconnected_items"}],
          "violations":[
            {"description":"Clearance violation","items":[{"description":"Track","pos":{"x":1.5,"y":2.5}},{"description":"Pad"}],"severity":"error","type":"clearance"},
            {"description":"Silkscreen overlap","items":[],"severity":"warning","type":"silk_overlap"}]}"#;
        let r = parse_drc(j, PathBuf::new()).unwrap();
        assert_eq!((r.errors, r.warnings, r.unconnected), (1, 1, 1));
        assert_eq!(r.items[0].pos, Some((1.5, 2.5)));
        assert_eq!(r.items[0].what.len(), 2);
        assert_eq!(r.items[2].kind, "unconnected");
    }
}
