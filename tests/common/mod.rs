//! Shared helpers for the integration tests.
// Helpers are shared by several test crates, each using a subset; tests may unwrap (see
// clippy.toml and the note in the test files).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use ict_jig_gen::app::{self, GenerateRequest, Outputs};
use ict_jig_gen::config::ConfigFile;
use ict_jig_gen::mesh::{Mesh, P3};
use ict_jig_gen::stl;

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn demo_zip() -> PathBuf {
    fixtures().join("demo_board_gerber.zip")
}

pub fn demo_dir() -> PathBuf {
    fixtures().join("demo_board")
}

pub fn demo_pnp() -> PathBuf {
    fixtures().join("PickAndPlace_demo_board.csv")
}

/// Run the pipeline on the demo zip with its Pick & Place file into a fresh temp dir.
pub fn generate_demo(config: ConfigFile) -> (Outputs, tempfile::TempDir) {
    let out = tempfile::tempdir().unwrap();
    let outputs = app::generate(&GenerateRequest {
        input: demo_zip(),
        out_dir: out.path().to_path_buf(),
        pnp: Some(demo_pnp()),
        config,
    })
    .unwrap();
    (outputs, out)
}

pub fn read_stl(path: &Path) -> Mesh {
    stl::read(&std::fs::read(path).unwrap()).unwrap()
}

/// Number of horizontal (normal = +/-Z) triangles whose XY footprint contains `(x, y)`.
pub fn flat_triangles_covering(mesh: &Mesh, x: f64, y: f64) -> usize {
    mesh.triangles()
        .filter(|t| t.iter().all(|v| (v[2] - t[0][2]).abs() < 1e-6))
        .filter(|t| point_in_triangle(x, y, t))
        .count()
}

fn point_in_triangle(x: f64, y: f64, t: &[P3; 3]) -> bool {
    let side = |a: &P3, b: &P3| (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
    let (d1, d2, d3) = (side(&t[0], &t[1]), side(&t[1], &t[2]), side(&t[2], &t[0]));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Distinct vertices at height `z` within `radius` of `(x, y)`.
pub fn vertices_near(mesh: &Mesh, x: f64, y: f64, z: f64, radius: f64) -> Vec<P3> {
    let mut out: Vec<P3> = Vec::new();
    for t in mesh.triangles() {
        for v in t {
            let close = (v[2] - z).abs() < 1e-4 && (v[0] - x).hypot(v[1] - y) <= radius;
            if close && !out.iter().any(|o| (o[0] - v[0]).hypot(o[1] - v[1]) < 1e-6) {
                out.push(v);
            }
        }
    }
    out
}

/// Write a minimal EasyEDA-like package folder: outline box + PTH + optional NPTH drills.
pub fn write_package(
    dir: &Path,
    bbox: (f64, f64, f64, f64),
    pth: &[(f64, f64)],
    npth: &[(f64, f64)],
) {
    let (x0, y0, x1, y1) = bbox;
    let g = |v: f64| format!("{}", (v * 1000.0).round());
    let gko = format!(
        "%FSLAX33Y33*%\n%MOMM*%\n%ADD10C,0.1*%\nD10*\nX{}Y{}D02*\nX{}Y{}D01*\nX{}Y{}D01*\nX{}Y{}D01*\nX{}Y{}D01*\nM02*\n",
        g(x0),
        g(y0),
        g(x1),
        g(y0),
        g(x1),
        g(y1),
        g(x0),
        g(y1),
        g(x0),
        g(y0)
    );
    std::fs::write(dir.join("Gerber_BoardOutlineLayer.GKO"), gko).unwrap();
    std::fs::write(dir.join("Drill_PTH_Through.DRL"), drill(0.9, pth)).unwrap();
    if !npth.is_empty() {
        std::fs::write(dir.join("Drill_NPTH_Through.DRL"), drill(2.2, npth)).unwrap();
    }
}

fn drill(dia: f64, pts: &[(f64, f64)]) -> String {
    let mut s = format!("M48\nMETRIC,TZ\nT1C{dia}\n%\nT1\n");
    for (x, y) in pts {
        writeln!(s, "X{x:.3}Y{y:.3}").unwrap();
    }
    s.push_str("M30\n");
    s
}

/// Zip the given `(name, bytes)` entries.
pub fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
    let mut zw = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in files {
        zw.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zw.write_all(bytes).unwrap();
    }
    zw.finish().unwrap();
}

/// PNG signature + IHDR claiming `w` x `h` (enough for dimension probing).
pub fn png_header(w: u32, h: u32) -> Vec<u8> {
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&w.to_be_bytes());
    png.extend_from_slice(&h.to_be_bytes());
    png.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
    png
}
