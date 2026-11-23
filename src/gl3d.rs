//! 3D view with components: the board model that `kicad-cli pcb export glb` writes is loaded once
//! and drawn with OpenGL at every board position of the panel, together with the panel body
//! (rails, tabs, backbones). The camera is the same orthographic one as in `view3d`, so everything
//! that module draws with the 2D painter (holes, text, V-cut lines, marks) lines up on top.

use crate::export::OutlineGeo;
use crate::panel::Panel;
use crate::view3d::Camera;
use eframe::egui::{self, Pos2, Rect};
use eframe::glow::{self, HasContext};
use std::sync::{Arc, Mutex};

type Pt = (f64, f64);

/// position (mm; x right, y down as in KiCad, z up), normal, colour (sRGB, straight alpha)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    pub col: [u8; 4],
}

#[derive(Default)]
pub struct MeshData {
    pub verts: Vec<Vertex>,
    pub opaque: Vec<u32>,
    /// triangles of see-through materials (solder mask), drawn last
    pub blend: Vec<u32>,
    /// highest / lowest point (mm)
    pub z_range: (f32, f32),
    /// solder mask colour, if the model has one
    pub mask: Option<[u8; 3]>,
}

// ------------------------------------------------------------------ GLB reader

type M4 = [[f64; 4]; 4]; // row major

const IDENT: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

fn mul(a: &M4, b: &M4) -> M4 {
    let mut m = [[0.0; 4]; 4];
    for r in 0..4 {
        for c in 0..4 {
            m[r][c] = (0..4).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    m
}

fn floats(v: Option<&serde_json::Value>, n: usize) -> Option<Vec<f64>> {
    let a = v?.as_array()?;
    if a.len() != n {
        return None;
    }
    a.iter().map(|x| x.as_f64()).collect()
}

/// local transform of a glTF node: `matrix`, or translation · rotation · scale
fn node_matrix(n: &serde_json::Value) -> M4 {
    if let Some(m) = floats(n.get("matrix"), 16) {
        // stored column major
        let mut r = IDENT;
        for (i, v) in m.iter().enumerate() {
            r[i % 4][i / 4] = *v;
        }
        return r;
    }
    let t = floats(n.get("translation"), 3).unwrap_or(vec![0.0; 3]);
    let q = floats(n.get("rotation"), 4).unwrap_or(vec![0.0, 0.0, 0.0, 1.0]);
    let s = floats(n.get("scale"), 3).unwrap_or(vec![1.0; 3]);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let rot = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ];
    let mut m = IDENT;
    for r in 0..3 {
        for c in 0..3 {
            m[r][c] = rot[r][c] * s[c];
        }
        m[r][3] = t[r];
    }
    m
}

fn srgb(linear: f64) -> u8 {
    let l = linear.clamp(0.0, 1.0);
    let s = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

struct Glb<'a> {
    json: serde_json::Value,
    bin: &'a [u8],
}

impl Glb<'_> {
    /// (bytes from the first element on, distance between elements, element count)
    fn accessor(&self, idx: u64, elem: usize) -> Result<(&[u8], usize, usize, u64), String> {
        let bad = || "3D model: broken accessor".to_string();
        let a = self.json["accessors"].get(idx as usize).ok_or_else(bad)?;
        if a.get("sparse").is_some() {
            return Err("3D model: sparse accessors are not supported".into());
        }
        let ctype = a["componentType"].as_u64().ok_or_else(bad)?;
        let count = a["count"].as_u64().ok_or_else(bad)? as usize;
        let v = self.json["bufferViews"].get(a["bufferView"].as_u64().ok_or_else(bad)? as usize).ok_or_else(bad)?;
        let size = elem * match ctype {
            5121 => 1,
            5123 => 2,
            _ => 4,
        };
        let stride = v["byteStride"].as_u64().map(|s| s as usize).filter(|s| *s >= size).unwrap_or(size);
        let start = (v["byteOffset"].as_u64().unwrap_or(0) + a["byteOffset"].as_u64().unwrap_or(0)) as usize;
        let need = if count == 0 { 0 } else { (count - 1) * stride + size };
        let data = self.bin.get(start..start.checked_add(need).ok_or_else(bad)?).ok_or_else(bad)?;
        Ok((data, stride, count, ctype))
    }

    fn vec3(&self, idx: u64) -> Result<Vec<[f32; 3]>, String> {
        let (d, stride, count, ctype) = self.accessor(idx, 3)?;
        if ctype != 5126 {
            return Err("3D model: positions are not stored as floats".into());
        }
        let f = |o: usize| f32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
        Ok((0..count).map(|i| [f(i * stride), f(i * stride + 4), f(i * stride + 8)]).collect())
    }

    fn indices(&self, idx: u64) -> Result<Vec<u32>, String> {
        let (d, stride, count, ctype) = self.accessor(idx, 1)?;
        Ok((0..count)
            .map(|i| {
                let o = i * stride;
                match ctype {
                    5121 => d[o] as u32,
                    5123 => u16::from_le_bytes([d[o], d[o + 1]]) as u32,
                    _ => u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]),
                }
            })
            .collect())
    }
}

/// Read a binary glTF file as written by KiCad (triangles, one colour per material, no textures)
/// into one mesh in board coordinates (mm).
pub fn load_glb(bytes: &[u8]) -> Result<MeshData, String> {
    let u32_at = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    if bytes.get(0..4) != Some(b"glTF") {
        return Err("3D model: not a GLB file".into());
    }
    let jlen = u32_at(12).ok_or("3D model: file too short")?;
    let jend = 20usize.checked_add(jlen).ok_or("3D model: broken header")?;
    let json: serde_json::Value =
        serde_json::from_slice(bytes.get(20..jend).ok_or("3D model: file too short")?).map_err(|e| format!("3D model: {e}"))?;
    let blen = u32_at(jend).ok_or("3D model: no geometry in the file")?;
    let bin = bytes.get(jend + 8..(jend + 8).checked_add(blen).ok_or("3D model: broken header")?).ok_or("3D model: file too short")?;
    let g = Glb { json, bin };

    let mut out = MeshData { z_range: (f32::MAX, f32::MIN), ..Default::default() };
    // copper colour of the 3D viewer: the metal material that is not grey
    let metal = |m: &serde_json::Value| m["pbrMetallicRoughness"]["metallicFactor"].as_f64().unwrap_or(0.0) > 0.5;
    let grey = |c: &[f64]| c[..3].iter().cloned().fold(0.0, f64::max) - c[..3].iter().cloned().fold(1.0, f64::min) < 0.08;
    let copper: Vec<f64> = g.json["materials"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| metal(m))
        .filter_map(|m| floats(m["pbrMetallicRoughness"].get("baseColorFactor"), 4))
        .find(|c| !grey(c))
        .unwrap_or(vec![0.7, 0.61, 0.0, 1.0]);
    let scene = g.json["scene"].as_u64().unwrap_or(0) as usize;
    let mut stack: Vec<(u64, M4)> =
        g.json["scenes"][scene]["nodes"].as_array().into_iter().flatten().filter_map(|n| n.as_u64()).map(|n| (n, IDENT)).collect();
    let mut visited = 0usize;
    while let Some((ni, parent)) = stack.pop() {
        visited += 1;
        if visited > 2_000_000 {
            return Err("3D model: node tree too large".into());
        }
        let Some(node) = g.json["nodes"].get(ni as usize) else { continue };
        let m = mul(&parent, &node_matrix(node));
        for c in node["children"].as_array().into_iter().flatten().filter_map(|c| c.as_u64()) {
            stack.push((c, m));
        }
        let Some(mesh) = node["mesh"].as_u64().and_then(|i| g.json["meshes"].get(i as usize)) else { continue };
        for prim in mesh["primitives"].as_array().into_iter().flatten() {
            if prim["mode"].as_u64().unwrap_or(4) != 4 {
                continue; // points / lines
            }
            let Some(pi) = prim["attributes"]["POSITION"].as_u64() else { continue };
            let pos = g.vec3(pi)?;
            let idx: Vec<u32> = match prim["indices"].as_u64() {
                Some(i) => g.indices(i)?,
                None => (0..pos.len() as u32).collect(),
            };
            if idx.iter().any(|i| *i as usize >= pos.len()) {
                return Err("3D model: index out of range".into());
            }
            let mut nrm = match prim["attributes"]["NORMAL"].as_u64() {
                Some(i) => g.vec3(i)?,
                None => Vec::new(),
            };
            if nrm.len() != pos.len() {
                // no normals stored: use the triangle planes
                nrm = vec![[0.0; 3]; pos.len()];
                for t in idx.chunks_exact(3) {
                    let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
                    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
                    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                    for i in t {
                        for k in 0..3 {
                            nrm[*i as usize][k] += n[k];
                        }
                    }
                }
            }
            let mat = prim["material"].as_u64().and_then(|i| g.json["materials"].get(i as usize));
            let mut rgba = mat.and_then(|m| floats(m["pbrMetallicRoughness"].get("baseColorFactor"), 4)).unwrap_or(vec![0.6, 0.6, 0.6, 1.0]);
            // the export paints pads tin grey; KiCad's 3D viewer shows them in the copper colour
            if mat.map(|m| metal(m)).unwrap_or(false) && grey(&rgba) {
                rgba = copper.clone();
            }
            let blend = mat.map(|m| m["alphaMode"] == "BLEND").unwrap_or(false) && rgba[3] < 0.995;
            // KiCad writes the board layers (body, copper, solder mask, silkscreen) with the colour
            // values of its 3D viewer, which shows them as they are; only these materials carry
            // metal / roughness values. Component models hold real linear colours.
            let pbr = mat.map(|m| &m["pbrMetallicRoughness"]);
            let layer = blend || pbr.map(|p| p.get("metallicFactor").is_some() || p.get("roughnessFactor").is_some()).unwrap_or(false);
            let conv = |v: f64| if layer { (v.clamp(0.0, 1.0) * 255.0).round() as u8 } else { srgb(v) };
            let col = [conv(rgba[0]), conv(rgba[1]), conv(rgba[2]), if blend { (rgba[3] * 255.0).round() as u8 } else { 255 }];
            if blend && out.mask.is_none() {
                out.mask = Some([col[0], col[1], col[2]]);
            }
            let base = out.verts.len() as u32;
            for (p, n) in pos.iter().zip(&nrm) {
                let (p, n) = ([p[0] as f64, p[1] as f64, p[2] as f64], [n[0] as f64, n[1] as f64, n[2] as f64]);
                let w: Vec<f64> = (0..3).map(|r| (m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3]) * 1000.0).collect();
                let d: Vec<f64> = (0..3).map(|r| m[r][0] * n[0] + m[r][1] * n[1] + m[r][2] * n[2]).collect();
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-12);
                // glTF: metres, y up, z towards the viewer  ->  board: mm, y = z, height = y
                let z = w[1] as f32;
                out.z_range = (out.z_range.0.min(z), out.z_range.1.max(z));
                out.verts.push(Vertex {
                    pos: [w[0] as f32, w[2] as f32, z],
                    nrm: [(d[0] / len) as f32, (d[2] / len) as f32, (d[1] / len) as f32],
                    col,
                });
            }
            let dst = if blend { &mut out.blend } else { &mut out.opaque };
            dst.extend(idx.chunks_exact(3).flatten().map(|i| base + i));
        }
    }
    if out.verts.is_empty() {
        return Err("3D model: the file contains no geometry".into());
    }
    Ok(out)
}

// ------------------------------------------------------------------ panel body

/// the panel frame (everything but the boards) as a slab: both faces and the side walls
pub fn slab(geo: &OutlineGeo, thickness: f64, face: [u8; 3]) -> MeshData {
    let half = (thickness / 2.0) as f32;
    let mut m = MeshData { z_range: (-half, half), ..Default::default() };
    let face = [face[0], face[1], face[2], 255];
    for (z, nz) in [(half, 1.0), (-half, -1.0)] {
        let base = m.verts.len() as u32;
        m.verts.extend(geo.verts.iter().map(|v| Vertex { pos: [v.0 as f32, v.1 as f32, z], nrm: [0.0, 0.0, nz], col: face }));
        m.opaque.extend(geo.tris.iter().flatten().map(|i| base + i));
    }
    let wall = [168, 160, 104, 255]; // bare FR4 edge
    for &(a, b) in &geo.walls {
        let (dx, dy) = ((b.0 - a.0) as f32, (b.1 - a.1) as f32);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-6 {
            continue;
        }
        let nrm = [dy / len, -dx / len, 0.0];
        let base = m.verts.len() as u32;
        for (q, z) in [(a, half), (b, half), (b, -half), (a, -half)] {
            m.verts.push(Vertex { pos: [q.0 as f32, q.1 as f32, z], nrm, col: wall });
        }
        m.opaque.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    m
}

/// colour of the panel faces: the model's solder mask over the board body, as KiCad shows it
pub fn face_colour(model: &MeshData) -> [u8; 3] {
    match model.mask {
        Some(c) => [0, 1, 2].map(|i| (c[i] as f32 * 0.83 + 77.0 * 0.17) as u8),
        None => [38, 110, 66],
    }
}

// ------------------------------------------------------------------ camera

/// column-major matrices for one draw call: position -> clip space, direction -> view space
#[derive(Clone, Copy)]
struct Draw {
    clip: [f32; 16],
    rot: [f32; 9],
}

/// where a board copy sits: 2D transform of the board file coordinates plus the height shift
#[derive(Clone, Copy)]
struct Place {
    /// x' = a·x + b·y + tx, y' = c·x + d·y + ty
    lin: [f64; 4],
    t: Pt,
    dz: f64,
}

const NO_MOVE: Place = Place { lin: [1.0, 0.0, 0.0, 1.0], t: (0.0, 0.0), dz: 0.0 };

pub struct View<'a> {
    pub cam: &'a Camera,
    /// pixels per mm
    pub zoom: f32,
    /// screen position of the panel center
    pub origin: Pos2,
    pub rect: Rect,
}

fn draw_matrices(v: &View, panel: (f64, f64), depth: f32, pl: &Place) -> Draw {
    let (sy, cy) = (v.cam.yaw.sin() as f64, v.cam.yaw.cos() as f64);
    let (sp, cp) = (v.cam.pitch.sin() as f64, v.cam.pitch.cos() as f64);
    // camera rows: right, up, towards the viewer (input: x, y down, z up) — same as view3d::Proj
    let cam = [[cy, sy, 0.0], [sy * sp, -cy * sp, cp], [-sy * cp, cy * cp, sp]];
    let [a, b, c, d] = pl.lin;
    let inst = [[a, b, 0.0], [c, d, 0.0], [0.0, 0.0, 1.0]];
    let t = [pl.t.0 - panel.0 / 2.0, pl.t.1 - panel.1 / 2.0, pl.dz];
    let mut r = [[0.0f64; 3]; 3];
    let mut tr = [0.0f64; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (0..3).map(|k| cam[i][k] * inst[k][j]).sum();
        }
        tr[i] = (0..3).map(|k| cam[i][k] * t[k]).sum();
    }
    let (zoom, hw, hh) = (v.zoom as f64, (v.rect.width() / 2.0) as f64, (v.rect.height() / 2.0) as f64);
    let off = v.origin - v.rect.center();
    // view space -> normalized device coordinates
    let s = [zoom / hw, zoom / hh, -1.0 / depth as f64];
    let o = [off.x as f64 / hw, -off.y as f64 / hh, 0.0];
    let mut clip = [0.0f32; 16];
    let mut rot = [0.0f32; 9];
    for row in 0..3 {
        for col in 0..3 {
            clip[col * 4 + row] = (r[row][col] * s[row]) as f32;
            rot[col * 3 + row] = r[row][col] as f32;
        }
        clip[12 + row] = (tr[row] * s[row] + o[row]) as f32;
    }
    clip[15] = 1.0;
    Draw { clip, rot }
}

// ------------------------------------------------------------------ OpenGL

struct GpuMesh {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    opaque: i32,
    blend: i32,
}

struct Gpu {
    program: glow::Program,
    u_clip: Option<glow::UniformLocation>,
    u_rot: Option<glow::UniformLocation>,
}

#[derive(Default)]
struct Inner {
    gpu: Option<Gpu>,
    model: Option<GpuMesh>,
    slab: Option<GpuMesh>,
    new_model: Option<Arc<MeshData>>,
    new_slab: Option<MeshData>,
    drop_model: bool,
    error: Option<String>,
}

const VS: &str = r#"#version 330
uniform mat4 u_clip;
uniform mat3 u_rot;
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_nrm;
layout(location = 2) in vec4 a_col;
out vec3 v_n;
out vec4 v_col;
void main() {
    gl_Position = u_clip * vec4(a_pos, 1.0);
    v_n = u_rot * a_nrm;
    v_col = a_col;
}
"#;

const FS: &str = r#"#version 330
in vec3 v_n;
in vec4 v_col;
out vec4 o_col;
void main() {
    vec3 n = normalize(v_n);
    if (n.z < 0.0) n = -n;               // both sides are lit
    vec3 l = normalize(vec3(-0.35, 0.55, 0.76));
    float diff = max(dot(n, l), 0.0);
    float spec = pow(max(dot(n, normalize(l + vec3(0.0, 0.0, 1.0))), 0.0), 24.0) * 0.16;
    o_col = vec4(v_col.rgb * (0.48 + 0.62 * diff) + spec, v_col.a);
}
"#;

unsafe fn compile(gl: &glow::Context) -> Result<Gpu, String> {
    let program = gl.create_program()?;
    let mut shaders = Vec::new();
    for (kind, src) in [(glow::VERTEX_SHADER, VS), (glow::FRAGMENT_SHADER, FS)] {
        let s = gl.create_shader(kind)?;
        gl.shader_source(s, src);
        gl.compile_shader(s);
        if !gl.get_shader_compile_status(s) {
            return Err(format!("shader: {}", gl.get_shader_info_log(s)));
        }
        gl.attach_shader(program, s);
        shaders.push(s);
    }
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        return Err(format!("shader: {}", gl.get_program_info_log(program)));
    }
    for s in shaders {
        gl.detach_shader(program, s);
        gl.delete_shader(s);
    }
    Ok(Gpu { program, u_clip: gl.get_uniform_location(program, "u_clip"), u_rot: gl.get_uniform_location(program, "u_rot") })
}

unsafe fn bytes<T>(v: &[T]) -> &[u8] {
    std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v))
}

unsafe fn upload(gl: &glow::Context, m: &MeshData) -> Result<GpuMesh, String> {
    let vao = gl.create_vertex_array()?;
    let vbo = gl.create_buffer()?;
    let ebo = gl.create_buffer()?;
    gl.bind_vertex_array(Some(vao));
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(&m.verts), glow::STATIC_DRAW);
    let stride = std::mem::size_of::<Vertex>() as i32;
    gl.enable_vertex_attrib_array(0);
    gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
    gl.enable_vertex_attrib_array(1);
    gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride, 12);
    gl.enable_vertex_attrib_array(2);
    gl.vertex_attrib_pointer_f32(2, 4, glow::UNSIGNED_BYTE, true, stride, 24);
    // opaque triangles first, see-through ones behind them
    let idx: Vec<u32> = m.opaque.iter().chain(&m.blend).copied().collect();
    gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
    gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes(&idx), glow::STATIC_DRAW);
    gl.bind_vertex_array(None);
    Ok(GpuMesh { vao, vbo, ebo, opaque: m.opaque.len() as i32, blend: m.blend.len() as i32 })
}

unsafe fn free(gl: &glow::Context, m: GpuMesh) {
    gl.delete_vertex_array(m.vao);
    gl.delete_buffer(m.vbo);
    gl.delete_buffer(m.ebo);
}

impl Inner {
    unsafe fn sync(&mut self, gl: &glow::Context) -> Result<(), String> {
        if self.gpu.is_none() {
            self.gpu = Some(compile(gl)?);
        }
        if self.drop_model {
            self.drop_model = false;
            if let Some(m) = self.model.take() {
                free(gl, m);
            }
        }
        if let Some(d) = self.new_model.take() {
            if let Some(m) = self.model.take() {
                free(gl, m);
            }
            self.model = Some(upload(gl, &d)?);
        }
        if let Some(d) = self.new_slab.take() {
            if let Some(m) = self.slab.take() {
                free(gl, m);
            }
            self.slab = Some(upload(gl, &d)?);
        }
        Ok(())
    }

    unsafe fn paint(&mut self, gl: &glow::Context, slab: &Draw, boards: &[Draw]) {
        if self.error.is_some() {
            return;
        }
        if let Err(e) = self.sync(gl) {
            self.error = Some(e);
            return;
        }
        let Some(gpu) = &self.gpu else { return };
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LEQUAL);
        gl.depth_mask(true);
        gl.clear_depth_f32(1.0);
        gl.clear(glow::DEPTH_BUFFER_BIT);
        gl.disable(glow::CULL_FACE);
        gl.disable(glow::BLEND);
        gl.use_program(Some(gpu.program));
        let set = |d: &Draw| {
            gl.uniform_matrix_4_f32_slice(gpu.u_clip.as_ref(), false, &d.clip);
            gl.uniform_matrix_3_f32_slice(gpu.u_rot.as_ref(), false, &d.rot);
        };
        if let Some(m) = &self.slab {
            gl.bind_vertex_array(Some(m.vao));
            set(slab);
            gl.draw_elements(glow::TRIANGLES, m.opaque, glow::UNSIGNED_INT, 0);
        }
        if let Some(m) = &self.model {
            gl.bind_vertex_array(Some(m.vao));
            for d in boards {
                set(d);
                gl.draw_elements(glow::TRIANGLES, m.opaque, glow::UNSIGNED_INT, 0);
            }
            if m.blend > 0 {
                gl.enable(glow::BLEND);
                gl.blend_func_separate(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::ZERO, glow::ONE);
                gl.depth_mask(false);
                for d in boards {
                    set(d);
                    gl.draw_elements(glow::TRIANGLES, m.blend, glow::UNSIGNED_INT, m.opaque * 4);
                }
                gl.depth_mask(true);
            }
        }
        gl.bind_vertex_array(None);
        gl.use_program(None);
        gl.disable(glow::DEPTH_TEST);
    }
}

/// handle kept by the GUI; the OpenGL objects live behind it and are touched only while painting
#[derive(Default)]
pub struct Renderer {
    inner: Arc<Mutex<Inner>>,
    /// what the uploaded panel body was built from
    slab_key: String,
    /// height range of the board model (mm, relative to the board's bottom face)
    z_range: (f32, f32),
    face: [u8; 3],
    has_model: bool,
}

impl Renderer {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_model(&mut self, m: MeshData) {
        self.z_range = m.z_range;
        self.face = face_colour(&m);
        self.has_model = true;
        self.slab_key.clear();
        self.lock().new_model = Some(Arc::new(m));
    }

    pub fn clear_model(&mut self) {
        self.has_model = false;
        let mut g = self.lock();
        g.new_model = None;
        g.drop_model = true;
    }

    pub fn has_model(&self) -> bool {
        self.has_model
    }

    /// set when OpenGL refused the shaders or buffers; the caller falls back to the plain view
    pub fn error(&self) -> Option<String> {
        self.lock().error.clone()
    }

    /// The paint job for one frame. `key` identifies the panel settings; `frame` builds the panel
    /// without the boards (`export::frame_geo`) and is called only when they changed.
    pub fn callback(&mut self, v: &View, panel: &Panel, frame: impl FnOnce() -> OutlineGeo, key: &str, thickness: f64) -> egui::PaintCallback {
        if self.slab_key != key {
            self.slab_key = key.to_string();
            self.lock().new_slab = Some(slab(&frame(), thickness, self.face));
        }
        let (pw, ph) = panel.size;
        let tall = self.z_range.0.abs().max(self.z_range.1.abs()) as f64 + thickness;
        let depth = ((pw * pw + ph * ph).sqrt() / 2.0 + tall) as f32 * 1.05;
        let slab_draw = draw_matrices(v, panel.size, depth, &NO_MOVE);
        let boards: Vec<Draw> = (0..panel.boards.len())
            .map(|i| {
                let (o, x, y) = (panel.to_panel(i, (0.0, 0.0)), panel.to_panel(i, (1.0, 0.0)), panel.to_panel(i, (0.0, 1.0)));
                let pl = Place { lin: [x.0 - o.0, y.0 - o.0, x.1 - o.1, y.1 - o.1], t: o, dz: -thickness / 2.0 };
                draw_matrices(v, panel.size, depth, &pl)
            })
            .collect();
        let inner = self.inner.clone();
        egui::PaintCallback {
            rect: v.rect,
            callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |_info, painter| {
                let mut g = inner.lock().unwrap_or_else(|e| e.into_inner());
                unsafe { g.paint(painter.gl(), &slab_draw, &boards) };
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// smallest useful GLB: one triangle under a node that is moved and turned
    fn tiny_glb(blend: bool) -> Vec<u8> {
        let mut bin: Vec<u8> = Vec::new();
        for p in [[0.0f32, 0.0, 0.0], [0.001, 0.0, 0.0], [0.0, 0.0, 0.002]] {
            for c in p {
                bin.extend(c.to_le_bytes());
            }
        }
        for i in [0u16, 1, 2, 0] {
            bin.extend(i.to_le_bytes());
        }
        let mat = if blend { r#"{"pbrMetallicRoughness":{"baseColorFactor":[0,1,0,0.5]},"alphaMode":"BLEND"}"# } else { r#"{"pbrMetallicRoughness":{"baseColorFactor":[1,0,0,1]}}"# };
        let mut json = format!(
            r#"{{"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"children":[1]}},{{"mesh":0,"translation":[0.01,0.0016,0.02]}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1,"material":0}}]}}],
            "materials":[{mat}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36,"byteStride":12}},{{"buffer":0,"byteOffset":36,"byteLength":8}}],
            "buffers":[{{"byteLength":44}}]}}"#
        )
        .into_bytes();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let mut out = b"glTF".to_vec();
        out.extend(2u32.to_le_bytes());
        out.extend(((28 + json.len() + bin.len()) as u32).to_le_bytes());
        out.extend((json.len() as u32).to_le_bytes());
        out.extend(b"JSON");
        out.extend(&json);
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(&bin);
        out
    }

    #[test]
    fn glb_to_board_coordinates() {
        let m = load_glb(&tiny_glb(false)).unwrap();
        assert_eq!((m.verts.len(), m.opaque.len(), m.blend.len()), (3, 3, 0));
        // metres -> mm, glTF z -> board y, glTF y -> height
        let close = |a: [f32; 3], b: [f32; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-3);
        assert!(close(m.verts[0].pos, [10.0, 20.0, 1.6]), "{:?}", m.verts[0].pos);
        assert!(close(m.verts[1].pos, [11.0, 20.0, 1.6]));
        assert!(close(m.verts[2].pos, [10.0, 22.0, 1.6]));
        // the triangle lies flat: its normal points up or down
        assert!(m.verts[0].nrm[2].abs() > 0.99, "{:?}", m.verts[0].nrm);
        assert_eq!(m.verts[0].col, [255, 0, 0, 255]);
        assert!(m.mask.is_none());
    }

    #[test]
    fn glb_see_through_material() {
        let m = load_glb(&tiny_glb(true)).unwrap();
        assert_eq!((m.opaque.len(), m.blend.len()), (0, 3));
        assert_eq!(m.verts[0].col, [0, 255, 0, 128]);
        assert_eq!(m.mask, Some([0, 255, 0]));
    }

    #[test]
    fn glb_rejects_broken_files() {
        assert!(load_glb(b"nope").is_err());
        let good = tiny_glb(false);
        assert!(load_glb(&good[..good.len() - 10]).is_err());
    }

    #[test]
    fn camera_matches_the_painter_view() {
        // straight from above, no pan: the panel center lands in the middle, x right, y down
        let cam = Camera { yaw: 0.0, pitch: std::f32::consts::FRAC_PI_2 };
        let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), egui::vec2(400.0, 200.0));
        let v = View { cam: &cam, zoom: 2.0, origin: rect.center(), rect };
        let d = draw_matrices(&v, (100.0, 50.0), 100.0, &NO_MOVE);
        let at = |p: [f32; 3]| -> [f32; 3] { [0, 1, 2].map(|r| d.clip[r] * p[0] + d.clip[4 + r] * p[1] + d.clip[8 + r] * p[2] + d.clip[12 + r]) };
        let c = at([50.0, 25.0, 0.0]);
        assert!(c[0].abs() < 1e-5 && c[1].abs() < 1e-5);
        // 50 mm to the right = 100 px = half of the half width
        assert!((at([100.0, 25.0, 0.0])[0] - 0.5).abs() < 1e-5);
        // further down on the board = lower on screen
        assert!(at([50.0, 50.0, 0.0])[1] < -0.1);
        // higher = closer to the viewer = smaller depth value
        assert!(at([50.0, 25.0, 5.0])[2] < at([50.0, 25.0, 0.0])[2]);
    }
}
