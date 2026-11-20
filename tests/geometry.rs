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
    let mesh = read_stl(&out.stl_paths[0]);
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

/// Regression: pillars in a row of three or more have collinear edges, which used to leave
/// T-junctions in the cavity floor/ceiling for some board sizes (e.g. 35.5 x 57.1 mm).
#[test]
fn watertight_for_a_sweep_of_board_sizes() {
    let sizes = [
        (35.5, 57.1),
        (20.0, 20.0),
        (31.0, 33.0),
        (47.9, 41.3),
        (60.0, 40.0),
        (64.2, 90.7),
        (88.8, 23.5),
        (120.0, 75.0),
        (150.5, 150.5),
    ];
    for (w, h) in sizes {
        let dir = tempfile::tempdir().unwrap();
        common::write_package(
            dir.path(),
            (0.0, 0.0, w, h),
            &[(10.0, 10.0), (14.0, 12.0)],
            &[],
        );
        let out = ict_jig_gen::app::generate(&ict_jig_gen::app::GenerateRequest {
            input: dir.path().to_path_buf(),
            out_dir: dir.path().join("out"),
            pnp: None,
            config: ConfigFile::default(),
        })
        .unwrap_or_else(|e| panic!("{w} x {h}: {e}"));
        let topo = read_stl(&out.stl_paths[0]).topology();
        assert!(topo.is_watertight(), "{w} x {h}: {topo:?}");
    }
}

// ------------------------------------------------------------------ --split

use ict_jig_gen::cassette::SCREW_PILOT_DIAMETER;
use ict_jig_gen::mesh::Mesh;

fn split_config() -> ConfigFile {
    ConfigFile {
        split: Some(true),
        ..ConfigFile::default()
    }
}

/// Generate the demo board split; returns the outputs and the (bottom, top) meshes.
fn split_demo(config: ConfigFile) -> (ict_jig_gen::app::Outputs, Mesh, Mesh, tempfile::TempDir) {
    let (out, dir) = generate_demo(config);
    let bottom = read_stl(&dir.path().join("cassette_bottom.stl"));
    let top = read_stl(&dir.path().join("cassette_top.stl"));
    (out, bottom, top, dir)
}

/// Number of downward-facing triangles that are *not* lying on the bed (`z = 0`):
/// anything above zero would need support material when printed in this orientation.
fn overhang_triangles(mesh: &Mesh) -> usize {
    mesh.triangles()
        .filter(|t| {
            let (u, v) = (
                [t[1][0] - t[0][0], t[1][1] - t[0][1]],
                [t[2][0] - t[0][0], t[2][1] - t[0][1]],
            );
            let nz = u[0] * v[1] - u[1] * v[0]; // z component of the (unnormalised) normal
            nz < -1e-9 && t.iter().any(|p| p[2].abs() > EPS)
        })
        .count()
}

#[test]
fn default_mode_still_writes_only_the_combined_stl() {
    let (out, dir) = generate_demo(ConfigFile::default());
    assert_eq!(out.stl_paths, vec![dir.path().join("cassette.stl")]);
    assert!(!dir.path().join("cassette_bottom.stl").exists());
    assert!(!dir.path().join("cassette_top.stl").exists());
    assert!(out.cassette.layout.joint.is_none());
    // the combined solid has a cavity ceiling, which is an overhang: the check can see them
    assert!(overhang_triangles(&read_stl(&out.stl_paths[0])) > 0);
}

#[test]
fn split_writes_two_files_and_no_combined_stl() {
    let (out, _b, _t, dir) = split_demo(split_config());
    assert_eq!(
        out.stl_paths,
        vec![
            dir.path().join("cassette_bottom.stl"),
            dir.path().join("cassette_top.stl")
        ]
    );
    assert!(!dir.path().join("cassette.stl").exists());
    assert!(dir.path().join("lista_elementow.json").exists());
    let guide = std::fs::read_to_string(dir.path().join("wiring_guide.txt")).unwrap();
    assert!(guide.contains("4x M3 x 10 mm") && guide.contains("cassette_top.stl"));
}

#[test]
fn split_halves_are_watertight_flat_on_z0_and_need_no_supports() {
    let (out, bottom, top, _d) = split_demo(split_config());
    let l = &out.cassette.layout;
    let joint = l.joint.as_ref().unwrap();
    for (name, mesh) in [("bottom", &bottom), ("top", &top)] {
        let topo = mesh.topology();
        assert!(topo.is_watertight(), "{name}: {topo:?}");
        assert!(mesh.signed_volume() > 500.0, "{name}: inside out?");
        let (lo, hi) = mesh.bounds().unwrap();
        assert!(close(lo[0], 0.0) && close(lo[1], 0.0), "{name}: {lo:?}");
        assert!(close(hi[0], 76.0) && close(hi[1], 56.0), "{name}: {hi:?}");
        assert!(close(lo[2], 0.0), "{name} must sit on Z = 0: {lo:?}");
        assert_eq!(overhang_triangles(mesh), 0, "{name} needs supports");
    }
    // bottom: plate 3 + gap 8 + boss; top: plate 3 + posts (1.6 pcb + 1.0)
    let (_, bhi) = bottom.bounds().unwrap();
    assert!(close(bhi[2], 11.0 + joint.boss_height), "{bhi:?}");
    let (_, thi) = top.bounds().unwrap();
    assert!(close(thi[2], 3.0 + 2.6), "{thi:?}");
    // together they are the same assembled height as the combined cassette
    assert!(close(
        l.z_top_plate_bottom + thi[2],
        out.cassette.layout.height
    ));
    // the boss hides inside the top plate and stops short of its surface
    assert!(joint.boss_height < 3.0 && joint.boss_height > 0.0);
}

#[test]
fn split_bores_stay_concentric_and_the_top_fits_over_the_bosses() {
    let (out, bottom, top, _d) = split_demo(split_config());
    let l = &out.cassette.layout;
    let joint = l.joint.as_ref().unwrap();
    let centroid = |ring: &[[f64; 3]]| {
        #[allow(clippy::cast_precision_loss)]
        let n = ring.len() as f64;
        (
            ring.iter().map(|v| v[0]).sum::<f64>() / n,
            ring.iter().map(|v| v[1]).sum::<f64>() / n,
        )
    };
    for hole in &l.holes {
        // the plate faces of each half that meet in the assembly: bottom z = 0 / 3, top z = 0 / 3
        // (top is printed at z = 0..3, assembled at z = 11..14)
        for (mesh, z) in [(&bottom, 0.0), (&bottom, 3.0), (&top, 0.0), (&top, 3.0)] {
            let ring = vertices_near(mesh, hole.x, hole.y, z, 1.0);
            assert_eq!(ring.len(), 32);
            let (cx, cy) = centroid(&ring);
            assert!(close(cx, hole.x) && close(cy, hole.y), "bore off-centre");
            for v in ring {
                assert!((v[0] - hole.x).hypot(v[1] - hole.y) >= 1.17 / 2.0 - EPS);
            }
        }
    }
    assert_eq!(joint.fasteners.len(), 4);
    for f in &joint.fasteners {
        let (x, y) = (f.center.x, f.center.y);
        // boss: ring at the pillar top (z = 11), inscribed in its nominal diameter
        let boss = vertices_near(&bottom, x, y, 11.0, 3.0);
        assert_eq!(boss.len(), 32, "boss ring");
        let boss_r = f.boss_diameter / 2.0;
        assert!(
            boss.iter()
                .all(|v| (v[0] - x).hypot(v[1] - y) <= boss_r + EPS)
        );
        // socket: through the whole top plate, never smaller than boss + clearance
        for z in [0.0, 3.0] {
            let socket = vertices_near(&top, x, y, z, 3.0);
            assert_eq!(socket.len(), 32, "socket at z = {z}");
            let min_r = socket
                .iter()
                .map(|v| (v[0] - x).hypot(v[1] - y))
                .fold(f64::MAX, f64::min);
            assert!(min_r >= f.boss_diameter / 2.0 + 0.075 - EPS, "{min_r}");
            assert!(min_r < boss_r + 0.3, "slop must stay a slip fit: {min_r}");
        }
        // blind pilot hole from the boss top down to the pillar base (z = 3)
        let pilot_top = vertices_near(&bottom, x, y, 11.0 + joint.boss_height, 1.6);
        assert_eq!(pilot_top.len(), 32);
        assert!(
            pilot_top
                .iter()
                .all(|v| (v[0] - x).hypot(v[1] - y) >= SCREW_PILOT_DIAMETER / 2.0 - EPS)
        );
        assert_eq!(vertices_near(&bottom, x, y, 3.0, 1.6).len(), 32);
        assert!(close(joint.pilot_depth, 11.0 + joint.boss_height - 3.0));
    }
}

#[test]
fn split_top_only_fits_one_way_round_at_any_clearance() {
    for clearance in [0.0, 0.1, 0.15, 0.2, 0.3] {
        let (out, _d) = generate_demo(ConfigFile {
            clearance: Some(clearance),
            ..split_config()
        });
        let joint = out.cassette.layout.joint.unwrap();
        let keyed: Vec<_> = joint.fasteners.iter().filter(|f| f.keyed).collect();
        assert_eq!(keyed.len(), 1);
        assert!(close(keyed[0].center.x, 4.0) && close(keyed[0].center.y, 4.0));
        for plain in joint.fasteners.iter().filter(|f| !f.keyed) {
            // the keyed boss must be clearly too big for an ordinary socket
            let socket = joint.socket_diameter(plain);
            assert!(
                keyed[0].boss_diameter >= socket + 0.25,
                "clearance {clearance}: keyed boss {} vs plain socket {socket}",
                keyed[0].boss_diameter
            );
            // ... and its walls around the pilot hole stay printable
            assert!((plain.boss_diameter - SCREW_PILOT_DIAMETER) / 2.0 >= 0.7 - 1e-9);
        }
        // the keyed boss still sits inside its 5 mm pillar
        assert!(keyed[0].boss_diameter <= 4.8 + 1e-9);
    }
}

#[test]
fn split_boss_walls_follow_min_wall_as_far_as_the_pillar_allows() {
    let boss = |min_wall: f64| {
        let (out, _d) = generate_demo(ConfigFile {
            min_wall: Some(min_wall),
            ..split_config()
        });
        let joint = out.cassette.layout.joint.unwrap();
        let plain = joint
            .fasteners
            .iter()
            .find(|f| !f.keyed)
            .unwrap()
            .boss_diameter;
        (plain, out.cassette.warnings)
    };
    // 0.8 mm walls (the default) fit: no warning
    let (plain, warnings) = boss(0.8);
    assert!(
        close(plain, SCREW_PILOT_DIAMETER + 1.6) && warnings.is_empty(),
        "{warnings:?}"
    );
    // 1.0 mm does not leave room for the key in a 5 mm pillar: capped, and said so
    let (plain, warnings) = boss(1.0);
    assert!(plain < SCREW_PILOT_DIAMETER + 2.0 && plain > SCREW_PILOT_DIAMETER + 1.4);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("thinner than --min-wall")),
        "{warnings:?}"
    );
}

#[test]
fn split_refuses_a_clearance_that_leaves_no_wall_around_the_screw() {
    let dir = tempfile::tempdir().unwrap();
    let err = ict_jig_gen::app::generate(&ict_jig_gen::app::GenerateRequest {
        input: common::demo_zip(),
        out_dir: dir.path().to_path_buf(),
        pnp: Some(common::demo_pnp()),
        config: ConfigFile {
            clearance: Some(0.6),
            ..split_config()
        },
    })
    .unwrap_err();
    assert!(err.to_string().contains("keyed locating boss"), "{err}");
}

#[test]
fn split_refuses_a_post_that_would_hit_a_screw_head() {
    let dir = tempfile::tempdir().unwrap();
    common::write_package(
        dir.path(),
        (0.0, 0.0, 40.0, 30.0),
        &[(20.0, 15.0), (22.54, 15.0)],
        &[(-0.8, -0.8), (40.8, 30.8)], // board.rs accepts holes just outside the outline
    );
    let request = |split: bool| ict_jig_gen::app::GenerateRequest {
        input: dir.path().to_path_buf(),
        out_dir: dir.path().join("out"),
        pnp: None,
        config: ConfigFile {
            margin: Some(7.0),
            alignment: Some(Alignment::Posts),
            split: Some(split),
            ..ConfigFile::default()
        },
    };
    ict_jig_gen::app::generate(&request(false)).unwrap();
    let err = ict_jig_gen::app::generate(&request(true)).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("too close to the M3 screw") && !msg.contains("internal error"),
        "{msg}"
    );
}

#[test]
fn switching_output_mode_warns_about_the_stale_files() {
    let dir = tempfile::tempdir().unwrap();
    let run = |split: bool| {
        ict_jig_gen::app::generate(&ict_jig_gen::app::GenerateRequest {
            input: common::demo_zip(),
            out_dir: dir.path().to_path_buf(),
            pnp: Some(common::demo_pnp()),
            config: ConfigFile {
                split: Some(split),
                ..ConfigFile::default()
            },
        })
        .unwrap()
    };
    let stale = |out: &ict_jig_gen::app::Outputs, name: &str| {
        out.warnings
            .iter()
            .any(|w| w.contains(name) && w.contains("earlier run"))
    };
    assert!(!stale(&run(false), "cassette"));
    let split = run(true);
    assert!(stale(&split, "cassette.stl"), "{:?}", split.warnings);
    assert!(split.guide.contains("earlier run"));
    let plain = run(false);
    assert!(stale(&plain, "cassette_bottom.stl"), "{:?}", plain.warnings);
    assert!(
        dir.path().join("cassette.stl").exists(),
        "files are never deleted"
    );
}

#[test]
fn split_with_a_lip_widens_the_margin_to_clear_the_screw_heads() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("pkg");
    std::fs::create_dir(&pkg).unwrap();
    common::write_package(
        &pkg,
        (0.0, 0.0, 40.0, 30.0),
        &[(10.0, 10.0), (12.54, 10.0)],
        &[],
    );
    let out = ict_jig_gen::app::generate(&ict_jig_gen::app::GenerateRequest {
        input: pkg,
        out_dir: dir.path().join("out"),
        pnp: None,
        config: split_config(),
    })
    .unwrap();
    let l = &out.cassette.layout;
    assert!(l.pocket.is_some());
    // screw centre 4.0 + lip corner 2.3 + head radius / sqrt 2 + 0.1 air
    let needed = 4.0 + 2.3 + 2.85 / std::f64::consts::SQRT_2 + 0.1;
    assert!(close(l.width, 40.0 + 2.0 * needed), "{}", l.width);
    assert!(
        out.warnings.iter().any(|w| w.contains("8.42 mm margin")),
        "{:?}",
        out.warnings
    );
    for path in &out.stl_paths {
        let mesh = read_stl(path);
        assert!(mesh.topology().is_watertight(), "{path:?}");
        assert_eq!(overhang_triangles(&mesh), 0, "{path:?}");
        assert!(close(mesh.bounds().unwrap().0[2], 0.0));
    }
    let (_, thi) = read_stl(&out.stl_paths[1]).bounds().unwrap();
    assert!(close(thi[2], 3.0 + 1.0), "lip crest: {thi:?}");
}

#[test]
fn split_pillars_too_short_for_a_screw_are_flagged() {
    let (out, _d) = generate_demo(ConfigFile {
        standoff_height: Some(2.0),
        ..split_config()
    });
    assert!(
        out.cassette
            .warnings
            .iter()
            .any(|w| w.contains("too short for a reliable M3 joint")),
        "{:?}",
        out.cassette.warnings
    );
    let (ok, _d) = generate_demo(split_config());
    assert!(
        ok.cassette.warnings.is_empty(),
        "{:?}",
        ok.cassette.warnings
    );
}

#[test]
fn split_is_watertight_and_flat_for_a_sweep_of_board_sizes() {
    for (w, h) in [
        (35.5, 57.1),
        (20.0, 20.0),
        (47.9, 41.3),
        (64.2, 90.7),
        (120.0, 75.0),
    ] {
        let dir = tempfile::tempdir().unwrap();
        common::write_package(
            dir.path(),
            (0.0, 0.0, w, h),
            &[(10.0, 10.0), (14.0, 12.0)],
            &[],
        );
        let out = ict_jig_gen::app::generate(&ict_jig_gen::app::GenerateRequest {
            input: dir.path().to_path_buf(),
            out_dir: dir.path().join("out"),
            pnp: None,
            config: split_config(),
        })
        .unwrap_or_else(|e| panic!("{w} x {h}: {e}"));
        assert_eq!(out.stl_paths.len(), 2);
        for path in &out.stl_paths {
            let mesh = read_stl(path);
            assert!(mesh.topology().is_watertight(), "{w} x {h} {path:?}");
            assert_eq!(overhang_triangles(&mesh), 0, "{w} x {h} {path:?}");
        }
    }
}

// ------------------------------------------------------------------ --compact

fn compact_config() -> ConfigFile {
    ConfigFile {
        compact: Some(true),
        ..ConfigFile::default()
    }
}

/// Run the pipeline on a synthetic package (outline box, PTH, 2.2 mm NPTH).
fn generate_package(
    bbox: (f64, f64, f64, f64),
    pth: &[(f64, f64)],
    npth: &[(f64, f64)],
    config: ConfigFile,
) -> (
    ict_jig_gen::Result<ict_jig_gen::app::Outputs>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("pkg");
    std::fs::create_dir(&pkg).unwrap();
    common::write_package(&pkg, bbox, pth, npth);
    let out = ict_jig_gen::app::generate(&ict_jig_gen::app::GenerateRequest {
        input: pkg,
        out_dir: dir.path().join("out"),
        pnp: None,
        config,
    });
    (out, dir)
}

/// Finite coordinates and no zero-area slivers (the watertight check only sees indices).
fn assert_no_degenerate_triangles(mesh: &Mesh, name: &str) {
    assert!(mesh.triangle_count() > 0, "{name}: empty");
    for t in mesh.triangles() {
        assert!(t.iter().flatten().all(|c| c.is_finite()), "{name}: {t:?}");
        let (u, v) = (
            [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]],
            [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]],
        );
        let area2 = ((u[1] * v[2] - u[2] * v[1]).powi(2)
            + (u[2] * v[0] - u[0] * v[2]).powi(2)
            + (u[0] * v[1] - u[1] * v[0]).powi(2))
        .sqrt();
        assert!(area2 > 1e-9, "{name}: zero-area triangle {t:?}");
    }
}

/// Distance of every vertex in `ring` from `(x, y)`: (min, max).
fn radii(ring: &[[f64; 3]], x: f64, y: f64) -> (f64, f64) {
    ring.iter()
        .map(|v| (v[0] - x).hypot(v[1] - y))
        .fold((f64::MAX, 0.0), |(lo, hi), r| (lo.min(r), hi.max(r)))
}

#[test]
fn compact_footprint_is_the_board_outline_with_pillars_on_the_mounting_holes() {
    let (out, dir) = generate_demo(compact_config());
    assert_eq!(out.stl_paths, vec![dir.path().join("cassette.stl")]);
    let mesh = read_stl(&out.stl_paths[0]);
    let topo = mesh.topology();
    assert!(topo.is_watertight(), "{topo:?}");
    assert_no_degenerate_triangles(&mesh, "compact");
    assert!(mesh.signed_volume() > 1000.0);
    // exactly the 60 x 40 mm board outline; Z as in the default cassette
    let (lo, hi) = mesh.bounds().unwrap();
    assert!(
        close(lo[0], 0.0) && close(lo[1], 0.0) && close(lo[2], 0.0),
        "{lo:?}"
    );
    assert!(
        close(hi[0], 60.0) && close(hi[1], 40.0) && close(hi[2], 16.6),
        "{hi:?}"
    );
    let l = &out.cassette.layout;
    assert_eq!(l.margin, 0.0);
    assert!(l.pillars.is_empty(), "no perimeter pillars");
    assert!(l.joint.is_none(), "no screws");
    assert_eq!(l.stepped_pillars.len(), 4);
    for (x, y) in [(5.0, 5.0), (55.0, 5.0), (5.0, 35.0), (55.0, 35.0)] {
        let s = l
            .stepped_pillars
            .iter()
            .find(|s| close(s.center.x, x) && close(s.center.y, y))
            .unwrap();
        assert!(close(s.pin_diameter, 2.05) && close(s.socket_diameter, 2.2));
        assert!(close(s.shoulder_diameter, 4.2));
        // the post above the top plate is the pillar's pin
        assert!(
            l.posts
                .iter()
                .any(|p| p.center == s.center && p.diameter == s.pin_diameter)
        );
        // shoulder: a 4.2 mm ring at the cavity floor (z = 3) and ceiling (z = 11)
        for z in [3.0, 11.0] {
            let ring = vertices_near(&mesh, x, y, z, 2.5);
            assert_eq!(ring.len(), 32, "shoulder at ({x}, {y}) z = {z}");
            let (rmin, rmax) = radii(&ring, x, y);
            assert!(rmax <= 2.1 + EPS && rmin >= 2.08, "{rmin} {rmax}");
        }
        // fused: inside the shoulder only the base and the top deck are horizontal faces
        assert_eq!(flat_triangles_covering(&mesh, x + 1.1, y + 1.03), 2);
        assert!(
            vertices_near(&mesh, x, y, 16.6, 2.0).len() >= 16,
            "post tip"
        );
    }
    // a solid point away from everything: base, cavity floor, cavity ceiling, deck
    assert_eq!(flat_triangles_covering(&mesh, 25.3, 24.7), 4);
    for (probe, hole) in out.pin_map.probes.iter().zip(&l.holes) {
        assert!(close(hole.x, probe.hole.pos.x - 100.0) && close(hole.y, probe.hole.pos.y - 50.0));
        assert_eq!(flat_triangles_covering(&mesh, hole.x, hole.y), 0);
        for z in [0.0, 3.0, 11.0, 14.0] {
            let ring = vertices_near(&mesh, hole.x, hole.y, z, 1.0);
            assert_eq!(ring.len(), 32, "pin {} z={z}", probe.pin);
            assert!(radii(&ring, hole.x, hole.y).0 >= 1.17 / 2.0 - EPS);
        }
    }
    let savings = out.cassette.savings.unwrap();
    assert!(close(savings.default_width, 76.0) && close(savings.default_depth, 56.0));
    assert!(close(savings.volume, mesh.signed_volume()));
    assert!(savings.volume < 0.7 * savings.default_volume, "{savings:?}");
    assert!(
        out.cassette.warnings.is_empty(),
        "{:?}",
        out.cassette.warnings
    );
    assert!(
        out.guide
            .contains("Compact   : footprint = board outline + 0.00 mm margin")
    );
    assert!(out.guide.contains("(-44 %)") && !out.guide.contains("no screws"));
}

#[test]
fn compact_split_halves_slide_together_without_screws() {
    let (out, bottom, top, dir) = split_demo(ConfigFile {
        split: Some(true),
        ..compact_config()
    });
    assert!(!dir.path().join("cassette.stl").exists());
    let l = &out.cassette.layout;
    assert!(l.joint.is_none() && l.pillars.is_empty());
    for (name, mesh) in [("bottom", &bottom), ("top", &top)] {
        let topo = mesh.topology();
        assert!(topo.is_watertight(), "{name}: {topo:?}");
        assert!(mesh.signed_volume() > 500.0, "{name}: inside out?");
        assert_no_degenerate_triangles(mesh, name);
        let (lo, hi) = mesh.bounds().unwrap();
        assert!(
            close(lo[0], 0.0) && close(lo[1], 0.0) && close(lo[2], 0.0),
            "{name}: {lo:?}"
        );
        assert!(close(hi[0], 60.0) && close(hi[1], 40.0), "{name}: {hi:?}");
        assert_eq!(overhang_triangles(mesh), 0, "{name} needs supports");
    }
    // bottom: up to the post tips; top: a flat 3 mm plate, assembled at z = 11..14
    assert!(close(bottom.bounds().unwrap().1[2], l.height));
    assert!(close(l.height, 14.0 + 2.6));
    assert!(close(top.bounds().unwrap().1[2], 3.0));
    assert!(close(l.z_top_plate_bottom + 3.0, l.z_top_plate_top));
    // nothing but the pins rises above the shoulders: no bosses, no perimeter pillars
    for v in bottom.triangles().flatten() {
        if v[2] > 11.0 + EPS {
            assert!(
                l.stepped_pillars
                    .iter()
                    .any(|s| (v[0] - s.center.x).hypot(v[1] - s.center.y)
                        <= s.pin_diameter / 2.0 + EPS),
                "{v:?} sticks up outside a pin"
            );
        }
    }
    for s in &l.stepped_pillars {
        let (x, y) = (s.center.x, s.center.y);
        // the ledge at z = 11: the shoulder ring outside, the pin ring inside
        let ledge = vertices_near(&bottom, x, y, 11.0, 2.5);
        assert_eq!(ledge.len(), 64);
        let (pin, shoulder): (Vec<_>, Vec<_>) =
            ledge.iter().partition(|v| (v[0] - x).hypot(v[1] - y) < 1.5);
        let (pin_min, pin_max) = radii(&pin, x, y);
        let (shoulder_min, _) = radii(&shoulder, x, y);
        assert!(pin_max <= s.pin_diameter / 2.0 + EPS && pin_min > 0.9);
        // the socket goes right through the top plate and is never smaller than pin + clearance
        let mut socket_max: f64 = 0.0;
        for z in [0.0, 3.0] {
            let ring = vertices_near(&top, x, y, z, 1.5);
            assert_eq!(ring.len(), 32, "socket at ({x}, {y}) z = {z}");
            let (rmin, rmax) = radii(&ring, x, y);
            assert!(rmin >= s.socket_diameter / 2.0 - EPS, "{rmin}");
            assert!(rmin < pin_max + 0.3, "slip fit, not slop: {rmin}");
            socket_max = socket_max.max(rmax);
        }
        assert_eq!(flat_triangles_covering(&top, x, y), 0, "socket is open");
        // the pin passes the socket, and the plate cannot slip past the shoulder
        assert!(pin_max < s.socket_diameter / 2.0);
        assert!(
            socket_max + 0.9 < shoulder_min,
            "{socket_max} vs {shoulder_min}"
        );
    }
    // the pogo bores of both halves line up
    for hole in &l.holes {
        for (mesh, z) in [(&bottom, 0.0), (&bottom, 3.0), (&top, 0.0), (&top, 3.0)] {
            let ring = vertices_near(mesh, hole.x, hole.y, z, 1.0);
            assert_eq!(ring.len(), 32);
            assert!(radii(&ring, hole.x, hole.y).0 >= 1.17 / 2.0 - EPS);
        }
    }
    assert!(
        out.cassette.warnings.is_empty(),
        "{:?}",
        out.cassette.warnings
    );
    assert!(out.guide.contains("no screws") && out.guide.contains("2.20 mm socket per pin"));
    assert!(!out.guide.contains("M3"), "{}", out.guide);
}

#[test]
fn compact_needs_two_mounting_holes() {
    let pth = [(10.0, 10.0), (14.0, 12.0)];
    for npth in [&[][..], &[(30.0, 20.0)][..]] {
        let (res, _d) = generate_package((0.0, 0.0, 40.0, 30.0), &pth, npth, compact_config());
        let msg = res.unwrap_err().to_string();
        assert!(
            msg.contains("--compact")
                && msg.contains("at least 2")
                && msg.contains("registration lip"),
            "{msg}"
        );
        // the default cassette copes
        let (res, _d) = generate_package((0.0, 0.0, 40.0, 30.0), &pth, npth, ConfigFile::default());
        res.unwrap();
    }
}

#[test]
fn compact_refuses_a_probe_that_hits_a_pillar_shoulder() {
    let npth = [(5.0, 5.0), (35.0, 25.0)];
    // 2.8 mm from a mounting hole: clear of a 2.05 mm post, but inside the 4.2 mm shoulder
    let pth = [(7.8, 5.0), (20.0, 15.0)];
    let (res, _d) = generate_package((0.0, 0.0, 40.0, 30.0), &pth, &npth, compact_config());
    let msg = res.unwrap_err().to_string();
    assert!(
        msg.contains("collides with the --compact pillar")
            && msg.contains("(5.00, 5.00)")
            && !msg.contains("internal error"),
        "{msg}"
    );
    let (res, _d) = generate_package((0.0, 0.0, 40.0, 30.0), &pth, &npth, ConfigFile::default());
    res.unwrap();
    // two mounting holes so close their shoulders would merge
    let (res, _d) = generate_package(
        (0.0, 0.0, 40.0, 30.0),
        &[(20.0, 15.0)],
        &[(10.0, 10.0), (14.0, 10.0)],
        compact_config(),
    );
    let msg = res.unwrap_err().to_string();
    assert!(msg.contains("would merge"), "{msg}");
}

#[test]
fn compact_widens_the_margin_only_as_far_as_the_board_edge_needs() {
    let check = |pth: &[(f64, f64)], npth: &[(f64, f64)], config, margin: f64, warn: &str| {
        let (res, _d) = generate_package((0.0, 0.0, 40.0, 30.0), pth, npth, config);
        let out = res.unwrap();
        let l = &out.cassette.layout;
        assert!(close(l.margin, margin), "{} vs {margin}", l.margin);
        assert!(close(l.width, 40.0 + 2.0 * margin) && close(l.depth, 30.0 + 2.0 * margin));
        let widened = out
            .cassette
            .warnings
            .iter()
            .find(|w| w.contains("--compact:"));
        match widened {
            Some(w) => assert!(!warn.is_empty() && w.contains(warn), "{w}"),
            None => assert!(warn.is_empty(), "no warning for {warn}"),
        }
        for path in &out.stl_paths {
            assert!(read_stl(path).topology().is_watertight(), "{path:?}");
        }
    };
    let inner = [(20.0, 15.0), (24.0, 15.0)];
    // mounting holes 2 mm from the edges: a 4.2 mm shoulder + 0.8 mm wall needs 0.9 mm more
    let near = [(2.0, 2.0), (38.0, 28.0)];
    check(
        &inner,
        &near,
        compact_config(),
        0.9,
        "the pillar on the mounting hole at (2.00, 2.00)",
    );
    // a pogo bore 0.2 mm from the edge needs 0.585 + 0.8 - 0.2 -> 1.2 mm
    let edge_pad = [(0.2, 15.0), (20.0, 15.0)];
    check(&edge_pad, &near, compact_config(), 1.2, "pin 0 (H");
    // nothing near the edge: the board outline exactly; an explicit margin is honoured
    let far = [(5.0, 5.0), (35.0, 25.0)];
    check(&inner, &far, compact_config(), 0.0, "");
    let explicit = ConfigFile {
        margin: Some(3.0),
        split: Some(true),
        ..compact_config()
    };
    check(&inner, &far, explicit, 3.0, "");
}

#[test]
fn compact_is_watertight_and_split_is_flat_for_a_sweep_of_board_sizes() {
    for (w, h) in [
        (20.0, 20.0),
        (35.5, 57.1),
        (47.9, 41.3),
        (64.2, 90.7),
        (120.0, 75.0),
    ] {
        let npth = [
            (4.0, 4.0),
            (w - 4.0, 4.0),
            (4.0, h - 4.0),
            (w - 4.0, h - 4.0),
        ];
        for split in [false, true] {
            let (res, _d) = generate_package(
                (0.0, 0.0, w, h),
                &[(10.0, 10.0), (14.0, 12.0)],
                &npth,
                ConfigFile {
                    split: Some(split),
                    ..compact_config()
                },
            );
            let out = res.unwrap_or_else(|e| panic!("{w} x {h} split={split}: {e}"));
            assert_eq!(out.stl_paths.len(), if split { 2 } else { 1 });
            for path in &out.stl_paths {
                let mesh = read_stl(path);
                assert!(mesh.topology().is_watertight(), "{w} x {h} {path:?}");
                assert_no_degenerate_triangles(&mesh, &format!("{w} x {h} {path:?}"));
                let (lo, hi) = mesh.bounds().unwrap();
                assert!(
                    close(hi[0] - lo[0], w) && close(hi[1] - lo[1], h),
                    "{w} x {h}"
                );
                assert!(close(lo[2], 0.0));
                if split {
                    assert_eq!(overhang_triangles(&mesh), 0, "{w} x {h} {path:?}");
                }
            }
        }
    }
}

#[test]
fn compact_split_on_pillars_in_one_line_warns_that_the_top_can_tip() {
    let pth = [(20.0, 22.0), (24.0, 22.0)];
    let in_a_row = [(5.0, 10.0), (20.0, 10.5), (35.0, 10.0)];
    let tip = |out: &ict_jig_gen::app::Outputs| {
        out.cassette
            .warnings
            .iter()
            .any(|w| w.contains("stand in one line"))
    };
    let split = ConfigFile {
        split: Some(true),
        ..compact_config()
    };
    let (res, _d) = generate_package((0.0, 0.0, 40.0, 30.0), &pth, &in_a_row, split.clone());
    assert!(tip(&res.unwrap()));
    // one-piece, the top plate is fused to the pillars
    let (res, _d) = generate_package((0.0, 0.0, 40.0, 30.0), &pth, &in_a_row, compact_config());
    assert!(!tip(&res.unwrap()));
    // a third hole off the line props the plate
    let triangle = [(5.0, 10.0), (35.0, 10.0), (20.0, 27.0)];
    let (res, _d) = generate_package((0.0, 0.0, 40.0, 30.0), &pth, &triangle, split);
    assert!(!tip(&res.unwrap()));
}
