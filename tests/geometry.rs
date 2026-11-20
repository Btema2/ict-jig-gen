//! Structural validation of the generated STL (the automated half of the `verify-stl` skill).
// Test code: the project's "no unwrap/panic" rule applies to library and binary code only
// (clippy.toml exempts #[test] fns; this extends it to the helpers in this test crate).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::many_single_char_names
)]

mod common;

use common::{flat_triangles_covering, generate_demo, read_stl, vertices_near};
use ict_jig_gen::config::{Alignment, ConfigFile};

const EPS: f64 = 1e-3; // f32 round trip through the STL

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

#[test]
fn bounding_box_matches_board_plus_margin_and_sits_on_z0() {
    let (out, dir) = generate_demo(ConfigFile::default());
    let mesh = read_stl(&dir.path().join("cassette.stl"));
    let (lo, hi) = mesh.bounds().unwrap();
    // 60 x 40 mm board + 2 x 8 mm margin; Z: 3 + 8 + 3 plates/gap + (1.6 pcb + 1.0) posts
    assert!(
        close(lo[0], 0.0) && close(lo[1], 0.0) && close(lo[2], 0.0),
        "{lo:?}"
    );
    assert!(close(hi[0], 76.0) && close(hi[1], 56.0), "{hi:?}");
    assert!(close(hi[2], 14.0 + 2.6), "{hi:?}");
    assert_eq!(out.cassette.layout.width, 76.0);
    assert_eq!(out.cassette.layout.depth, 56.0);
    // the flat base: triangles lying in z = 0 tile the footprint minus the 13 bores
    let base_area: f64 = mesh
        .triangles()
        .filter(|t| t.iter().all(|v| v[2].abs() < EPS))
        .map(|t| {
            0.5 * ((t[1][0] - t[0][0]) * (t[2][1] - t[0][1])
                - (t[2][0] - t[0][0]) * (t[1][1] - t[0][1]))
                .abs()
        })
        .sum();
    let bore = out.cassette.layout.hole_diameter / 2.0 / (std::f64::consts::PI / 32.0).cos();
    let bore_area = 0.5 * 32.0 * bore * bore * (std::f64::consts::TAU / 32.0).sin();
    assert!(
        (base_area - (76.0 * 56.0 - 13.0 * bore_area)).abs() < 0.05,
        "base area {base_area}"
    );
}

#[test]
fn mesh_is_a_valid_closed_solid() {
    let (_, dir) = generate_demo(ConfigFile::default());
    let mesh = read_stl(&dir.path().join("cassette.stl"));
    let topo = mesh.topology();
    assert!(topo.triangles > 1000);
    assert!(topo.is_watertight(), "{topo:?}");
    assert!(
        mesh.signed_volume() > 1000.0,
        "normals point inwards? volume {}",
        mesh.signed_volume()
    );
    for t in mesh.triangles() {
        assert!(t.iter().flatten().all(|c| c.is_finite()));
        let (u, v) = (
            [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]],
            [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]],
        );
        let area2 = ((u[1] * v[2] - u[2] * v[1]).powi(2)
            + (u[2] * v[0] - u[0] * v[2]).powi(2)
            + (u[0] * v[1] - u[1] * v[0]).powi(2))
        .sqrt();
        assert!(area2 > 1e-9, "zero-area triangle {t:?}");
    }
}

#[test]
fn pogo_holes_are_real_through_holes_at_the_probe_positions() {
    let (out, dir) = generate_demo(ConfigFile::default());
    let mesh = read_stl(&dir.path().join("cassette.stl"));
    let l = &out.cassette.layout;
    assert_eq!(l.holes.len(), 13);
    // a solid control point is covered by the base, both plate faces and the top
    assert_eq!(flat_triangles_covering(&mesh, 20.0, 45.0), 4);
    for (probe, hole) in out.pin_map.probes.iter().zip(&l.holes) {
        // cassette position = board position + (margin - board min)
        assert!(
            close(hole.x, probe.hole.pos.x - 100.0 + 8.0)
                && close(hole.y, probe.hole.pos.y - 50.0 + 8.0)
        );
        assert_eq!(
            flat_triangles_covering(&mesh, hole.x, hole.y),
            0,
            "pin {} is blocked",
            probe.pin
        );
        // concentric bores through both plates: ring vertices on all four plate faces
        for z in [0.0, 3.0, 11.0, 14.0] {
            let ring = vertices_near(&mesh, hole.x, hole.y, z, 1.0);
            assert_eq!(ring.len(), 32, "pin {} z={z}", probe.pin);
            for v in ring {
                // bore never narrower than the 1.02 mm barrel (+0.15 clearance)
                assert!((v[0] - hole.x).hypot(v[1] - hole.y) >= 1.17 / 2.0 - EPS);
            }
        }
    }
}

#[test]
fn alignment_posts_stand_at_the_mounting_holes() {
    let (out, dir) = generate_demo(ConfigFile::default());
    let mesh = read_stl(&dir.path().join("cassette.stl"));
    let posts = &out.cassette.layout.posts;
    assert_eq!(posts.len(), 4);
    for (x, y) in [(13.0, 13.0), (63.0, 13.0), (13.0, 43.0), (63.0, 43.0)] {
        let post = posts
            .iter()
            .find(|p| close(p.center.x, x) && close(p.center.y, y))
            .unwrap();
        assert!(
            close(post.diameter, 2.2 - 0.15),
            "post must be undersized for the 2.2 mm hole"
        );
        let tip = vertices_near(&mesh, x, y, 16.6, 2.0);
        assert!(
            tip.len() >= 16,
            "post at ({x}, {y}) has no tip: {}",
            tip.len()
        );
        assert!(flat_triangles_covering(&mesh, x, y) >= 1);
    }
    assert!(
        out.cassette.layout.pocket.is_none(),
        "posts present, so no lip in auto mode"
    );
}

#[test]
fn registration_lip_is_used_when_the_board_has_no_mounting_holes() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("pkg");
    std::fs::create_dir(&pkg).unwrap();
    common::write_package(
        &pkg,
        (0.0, 0.0, 40.0, 30.0),
        &[(10.0, 10.0), (12.54, 10.0), (30.0, 20.0)],
        &[],
    );
    let out = ict_jig_gen::app::generate(&ict_jig_gen::app::GenerateRequest {
        input: pkg,
        out_dir: dir.path().join("out"),
        pnp: None,
        config: ConfigFile::default(),
    })
    .unwrap();
    let l = &out.cassette.layout;
    let pocket = l.pocket.expect("lip expected");
    assert!(close(pocket.max.x - pocket.min.x, 40.6) && close(pocket.max.y - pocket.min.y, 30.6));
    assert!(l.posts.is_empty());
    let mesh = read_stl(&out.stl_path);
    let (lo, hi) = mesh.bounds().unwrap();
    assert!(close(lo[2], 0.0) && close(hi[2], 14.0 + 1.0), "{hi:?}");
    assert!(close(hi[0], 56.0) && close(hi[1], 46.0));
    assert!(mesh.topology().is_watertight());
    // the seat inside the lip is lower than the lip crest, and the holes still go through
    assert!(flat_triangles_covering(&mesh, 8.0 + 10.0, 8.0 + 10.0) == 0);
    assert!(flat_triangles_covering(&mesh, 28.0, 21.0) >= 1);
}

#[test]
fn every_alignment_mode_yields_a_watertight_solid() {
    for mode in [
        Alignment::Auto,
        Alignment::Posts,
        Alignment::Lip,
        Alignment::Both,
        Alignment::None,
    ] {
        let (_, dir) = generate_demo(ConfigFile {
            alignment: Some(mode),
            ..ConfigFile::default()
        });
        let topo = read_stl(&dir.path().join("cassette.stl")).topology();
        assert!(topo.is_watertight(), "{mode:?}: {topo:?}");
    }
}

#[test]
fn switching_to_p100_e2_widens_the_holes() {
    let (out, dir) = generate_demo(ConfigFile {
        pogo: Some("P100-E2".into()),
        ..ConfigFile::default()
    });
    assert!(close(out.cassette.layout.hole_diameter, 1.36 + 0.15));
    let mesh = read_stl(&dir.path().join("cassette.stl"));
    let h = out.cassette.layout.holes[0];
    let ring = vertices_near(&mesh, h.x, h.y, 0.0, 2.0);
    assert!(
        ring.iter()
            .all(|v| (v[0] - h.x).hypot(v[1] - h.y) >= 1.51 / 2.0 - EPS)
    );
    assert!(mesh.topology().is_watertight());
}

#[test]
fn mechanical_overrides_change_the_geometry() {
    let cfg = ConfigFile {
        plate_thickness: Some(2.0),
        standoff_height: Some(6.0),
        margin: Some(10.0),
        clearance: Some(0.3),
        ..ConfigFile::default()
    };
    let (out, dir) = generate_demo(cfg);
    let (lo, hi) = read_stl(&dir.path().join("cassette.stl")).bounds().unwrap();
    assert!(close(hi[0], 80.0) && close(hi[1], 60.0), "{hi:?}");
    assert!(close(hi[2], 10.0 + 2.6) && close(lo[2], 0.0), "{hi:?}");
    assert!(close(out.cassette.layout.hole_diameter, 1.32));
}

#[test]
fn stack_up_that_starves_the_plunger_is_flagged() {
    let (out, _d) = generate_demo(ConfigFile {
        standoff_height: Some(12.0),
        ..ConfigFile::default()
    });
    assert!(
        out.cassette
            .warnings
            .iter()
            .any(|w| w.contains("plunger rises only")),
        "{:?}",
        out.cassette.warnings
    );
    let (ok, _d) = generate_demo(ConfigFile::default());
    assert!(
        ok.cassette.warnings.is_empty(),
        "{:?}",
        ok.cassette.warnings
    );
}
