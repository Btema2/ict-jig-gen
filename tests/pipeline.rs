//! End-to-end behaviour: inputs, pin map contract, error handling and the CLI binary.
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

use std::process::Command;

use common::{demo_dir, demo_pnp, demo_zip, generate_demo, make_zip, png_header, write_package};
use ict_jig_gen::app::{self, GenerateRequest};
use ict_jig_gen::config::{ConfigFile, ElementSpec, Typ};
use serde_json::Value;

fn request(input: &std::path::Path, out: &std::path::Path, config: ConfigFile) -> GenerateRequest {
    GenerateRequest {
        input: input.to_path_buf(),
        out_dir: out.to_path_buf(),
        pnp: None,
        config,
    }
}

fn json_of(dir: &tempfile::TempDir) -> Value {
    serde_json::from_slice(&std::fs::read(dir.path().join("lista_elementow.json")).unwrap())
        .unwrap()
}

fn with_rail() -> ConfigFile {
    ConfigFile {
        element: vec![ElementSpec::parse_pair("VCC-GND=4,5", Typ::Linia).unwrap()],
        ..ConfigFile::default()
    }
}

// ---------------------------------------------------------------- JSON contract

#[test]
fn lista_elementow_matches_the_contract_exactly() {
    let (out, dir) = generate_demo(with_rail());
    let text = std::fs::read_to_string(dir.path().join("lista_elementow.json")).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    let top = v.as_object().expect("top level must be an object");
    assert_eq!(top.keys().collect::<Vec<_>>(), ["elementy"]);
    let list = top["elementy"].as_array().unwrap();
    assert_eq!(list.len(), out.lista.elementy.len());
    assert_eq!(list.len(), 6); // D1 D2 J1 R1 R2 + VCC-GND
    for entry in list {
        let o = entry.as_object().unwrap();
        let mut keys: Vec<_> = o.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["nazwa", "piny", "typ", "x", "y"], "{entry}");
        assert!(o["nazwa"].as_str().is_some_and(|s| !s.trim().is_empty()));
        let pins = o["piny"].as_array().unwrap();
        assert_eq!(pins.len(), 2);
        let (a, b) = (pins[0].as_u64().unwrap(), pins[1].as_u64().unwrap());
        assert!(a <= 15 && b <= 15 && a != b, "{entry}");
        assert!(
            o["x"].is_u64() && o["y"].is_u64(),
            "pixels must be integers: {entry}"
        );
        assert!(o["x"].as_u64().unwrap() < 512 && o["y"].as_u64().unwrap() < 512);
        assert!(matches!(o["typ"].as_str(), Some("element" | "linia")));
    }
    let rail = list.iter().find(|e| e["nazwa"] == "VCC-GND").unwrap();
    assert_eq!(rail["typ"], "linia");
    assert_eq!(rail["piny"], serde_json::json!([4, 5]));
    assert!(text.ends_with("}\n"));
    assert!(!text.contains("\\u"), "UTF-8 must not be escaped");
}

#[test]
fn pixels_map_linearly_from_the_outline_with_y_flipped() {
    let (_, dir) = generate_demo(ConfigFile::default());
    let v = json_of(&dir);
    let find = |n: &str| {
        v["elementy"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["nazwa"] == n)
            .unwrap()
            .clone()
    };
    // board (100,50)-(160,90) -> 512x512; R1 centroid (115, 80): x = 15/60*512, y = (90-80)/40*512
    let r1 = find("R1");
    assert_eq!((r1["x"].as_u64(), r1["y"].as_u64()), (Some(128), Some(128)));
    // D2 centroid (135, 70): x = 35/60*512 = 298.67 -> 299, y = 20/40*512 = 256
    let d2 = find("D2");
    assert_eq!((d2["x"].as_u64(), d2["y"].as_u64()), (Some(299), Some(256)));
    // higher on the board (larger Gerber Y) means a smaller image Y
    assert!(find("R1")["y"].as_u64() < find("R2")["y"].as_u64());
    assert!(find("R2")["y"].as_u64() < find("J1")["y"].as_u64());
}

#[test]
fn image_size_can_be_explicit_or_read_from_a_png() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("zdjecie_plytki.png");
    std::fs::write(&png, png_header(1200, 800)).unwrap();
    let from_png = app::generate(&GenerateRequest {
        pnp: Some(demo_pnp()),
        ..request(
            &demo_zip(),
            &dir.path().join("a"),
            ConfigFile {
                image: Some(png.clone()),
                ..ConfigFile::default()
            },
        )
    })
    .unwrap();
    let r1 = from_png
        .lista
        .elementy
        .iter()
        .find(|e| e.nazwa == "R1")
        .unwrap();
    assert_eq!((r1.x, r1.y), (300, 200)); // 15/60*1200, 10/40*800
    assert!(
        from_png.warnings.iter().all(|w| !w.contains("stretched")),
        "aspect matches, no warning"
    );

    let explicit = app::generate(&GenerateRequest {
        pnp: Some(demo_pnp()),
        ..request(
            &demo_zip(),
            &dir.path().join("b"),
            // explicit size wins over --image
            ConfigFile {
                image: Some(png),
                image_size: Some("600x400".into()),
                ..ConfigFile::default()
            },
        )
    })
    .unwrap();
    let r1 = explicit
        .lista
        .elementy
        .iter()
        .find(|e| e.nazwa == "R1")
        .unwrap();
    assert_eq!((r1.x, r1.y), (150, 100)); // 15/60*600, 10/40*400
}

#[test]
fn mismatched_image_aspect_is_warned_about() {
    let (out, _d) = generate_demo(ConfigFile::default()); // 512x512 vs 60x40 board
    assert!(
        out.warnings.iter().any(|w| w.contains("stretched")),
        "{:?}",
        out.warnings
    );
}

// ---------------------------------------------------------------- inputs

#[test]
fn zip_and_unzipped_directory_give_identical_results() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let cfg = || ConfigFile::default();
    // the unzipped folder contains PickAndPlace_*.csv, so it is discovered automatically
    app::generate(&request(&demo_dir(), a.path(), cfg())).unwrap();
    app::generate(&GenerateRequest {
        pnp: Some(demo_pnp()),
        ..request(&demo_zip(), b.path(), cfg())
    })
    .unwrap();
    for f in ["lista_elementow.json", "cassette.stl", "wiring_guide.txt"] {
        let (x, y) = (
            std::fs::read(a.path().join(f)).unwrap(),
            std::fs::read(b.path().join(f)).unwrap(),
        );
        // the guide embeds the input path, everything else must match byte for byte
        if f != "wiring_guide.txt" {
            assert!(x == y, "{f} differs between zip and directory input");
        }
    }
}

#[test]
fn pick_and_place_inside_the_zip_is_discovered() {
    let dir = tempfile::tempdir().unwrap();
    let zip = dir.path().join("with_pnp.zip");
    let read = |n: &str| std::fs::read(demo_dir().join(n)).unwrap();
    let (gko, pth, npth, pnp) = (
        read("Gerber_BoardOutlineLayer.GKO"),
        read("Drill_PTH_Through.DRL"),
        read("Drill_NPTH_Through.DRL"),
        read("PickAndPlace_demo_board.csv"),
    );
    make_zip(
        &zip,
        &[
            ("export/Gerber_BoardOutlineLayer.GKO", &gko),
            ("export/Drill_PTH_Through.DRL", &pth),
            ("export/Drill_NPTH_Through.DRL", &npth),
            ("export/PickAndPlace_demo_board.csv", &pnp),
            ("__MACOSX/export/._Drill_PTH_Through.DRL", b"junk"),
        ],
    );
    let out = app::generate(&request(&zip, &dir.path().join("o"), ConfigFile::default())).unwrap();
    assert!(out.lista.elementy.iter().any(|e| e.nazwa == "R1"));
    assert_eq!(out.board.components.len(), 6);
}

#[test]
fn utf16_pick_and_place_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let text = std::fs::read_to_string(demo_pnp()).unwrap();
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    let csv = dir.path().join("PickAndPlace_utf16.csv");
    std::fs::write(&csv, bytes).unwrap();
    let out = app::generate(&GenerateRequest {
        pnp: Some(csv),
        ..request(&demo_zip(), &dir.path().join("o"), ConfigFile::default())
    })
    .unwrap();
    assert!(out.lista.elementy.iter().any(|e| e.nazwa == "D1"));
}

#[test]
fn without_pick_and_place_elements_need_explicit_definitions() {
    let dir = tempfile::tempdir().unwrap();
    let out = app::generate(&request(&demo_zip(), dir.path(), ConfigFile::default())).unwrap();
    assert!(out.lista.elementy.is_empty());
    assert!(
        out.warnings
            .iter()
            .any(|w| w.contains("no elements defined"))
    );
    assert_eq!(
        out.pin_map.probes.len(),
        13,
        "pins are still assigned in reading order"
    );
    let cfg = ConfigFile {
        probe: vec!["0=H1".parse().unwrap(), "1=H2".parse().unwrap()],
        element: vec![ElementSpec::parse_pair("R1=0,1", Typ::Element).unwrap()],
        ..ConfigFile::default()
    };
    let out = app::generate(&request(&demo_zip(), &dir.path().join("x"), cfg)).unwrap();
    assert_eq!(out.lista.elementy.len(), 1);
    assert_eq!(out.cassette.layout.holes.len(), 2);
}

// ---------------------------------------------------------------- pin selection / physics

fn grid(n: u32, pitch: f64) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            (
                10.0 + f64::from(i % 10) * pitch,
                10.0 + f64::from(i / 10) * pitch,
            )
        })
        .collect()
}

#[test]
fn more_than_sixteen_holes_need_an_explicit_selection() {
    let dir = tempfile::tempdir().unwrap();
    write_package(
        dir.path(),
        (0.0, 0.0, 60.0, 40.0),
        &grid(20, 4.0),
        &[(3.0, 3.0), (57.0, 37.0)],
    );
    let err = app::generate(&request(
        dir.path(),
        &dir.path().join("o"),
        ConfigFile::default(),
    ))
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("20 plated holes") && err.contains("16 channels") || err.contains("only 16"),
        "{err}"
    );

    let probes = (0..16u8)
        .map(|p| format!("{p}=H{}", u32::from(p) + 3).parse().unwrap())
        .collect();
    let out = app::generate(&request(
        dir.path(),
        &dir.path().join("o2"),
        ConfigFile {
            probe: probes,
            ..ConfigFile::default()
        },
    ))
    .unwrap();
    assert_eq!(out.cassette.layout.holes.len(), 16);
    assert_eq!(out.pin_map.probes.last().unwrap().pin, 15);
    // 16 probes among 20 holes: only the chosen holes are drilled in the cassette
    let mesh = common::read_stl(&out.stl_paths[0]);
    assert!(mesh.topology().is_watertight());
}

#[test]
fn pin_out_of_range_and_duplicate_assignments_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = ConfigFile {
        probe: vec!["16=H1".parse().unwrap()],
        ..ConfigFile::default()
    };
    assert!(
        app::generate(&request(&demo_zip(), dir.path(), cfg.clone()))
            .unwrap_err()
            .to_string()
            .contains("out of range")
    );
    cfg.probe = vec!["1=H1".parse().unwrap(), "1=H2".parse().unwrap()];
    assert!(
        app::generate(&request(&demo_zip(), dir.path(), cfg.clone()))
            .unwrap_err()
            .to_string()
            .contains("assigned twice")
    );
    cfg.probe = vec!["1=H1".parse().unwrap()];
    cfg.element = vec![ElementSpec::parse_pair("X=1,2", Typ::Element).unwrap()];
    assert!(
        app::generate(&request(&demo_zip(), dir.path(), cfg))
            .unwrap_err()
            .to_string()
            .contains("no probe assigned")
    );
}

#[test]
fn holes_too_close_for_the_pogo_are_rejected_with_a_clear_message() {
    let dir = tempfile::tempdir().unwrap();
    write_package(
        dir.path(),
        (0.0, 0.0, 40.0, 30.0),
        &[(10.0, 10.0), (11.6, 10.0), (30.0, 20.0)],
        &[],
    );
    let err = app::generate(&request(
        dir.path(),
        &dir.path().join("o"),
        ConfigFile::default(),
    ))
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("1.60 mm apart") && err.contains("1.97 mm"),
        "{err}"
    );
    assert!(err.contains("pins 1 (H2) and 2 (H3)"), "{err}"); // pin 0 is the top-most hole
    // a wider barrel makes an otherwise fine pitch impossible
    let ok = tempfile::tempdir().unwrap();
    write_package(
        ok.path(),
        (0.0, 0.0, 40.0, 30.0),
        &[(10.0, 10.0), (12.2, 10.0)],
        &[],
    ); // 2.2 mm pitch: fits 1.17 mm holes (1.97), not 1.51 mm (2.31)
    assert!(
        app::generate(&request(
            ok.path(),
            &ok.path().join("o"),
            ConfigFile::default()
        ))
        .is_ok()
    );
    let wide = ConfigFile {
        pogo: Some("P100-E2".into()),
        ..ConfigFile::default()
    };
    assert!(app::generate(&request(ok.path(), &ok.path().join("o2"), wide)).is_err());
}

#[test]
fn test_point_at_the_board_edge_cannot_fit_inside_the_lip() {
    let dir = tempfile::tempdir().unwrap();
    write_package(
        dir.path(),
        (0.0, 0.0, 40.0, 30.0),
        &[(0.2, 15.0), (20.0, 15.0)],
        &[],
    );
    let err = app::generate(&request(
        dir.path(),
        &dir.path().join("o"),
        ConfigFile::default(),
    ))
    .unwrap_err()
    .to_string();
    assert!(err.contains("too close to the board edge"), "{err}");
}

// ---------------------------------------------------------------- invalid input never panics

#[test]
fn malformed_and_missing_inputs_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    let run = |input: &std::path::Path| {
        app::generate(&request(input, &out, ConfigFile::default()))
            .map(|_| ())
            .expect_err("input should be rejected")
            .to_string()
    };

    assert!(run(&dir.path().join("nope.zip")).contains("I/O error"));
    let empty = dir.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    assert!(run(&empty).contains("no board outline"));

    let not_zip = dir.path().join("fake.zip");
    std::fs::write(&not_zip, b"this is not a zip archive").unwrap();
    assert!(run(&not_zip).contains("cannot read archive"));

    let no_drill = dir.path().join("no_drill.zip");
    make_zip(
        &no_drill,
        &[(
            "Gerber_BoardOutlineLayer.GKO",
            &std::fs::read(demo_dir().join("Gerber_BoardOutlineLayer.GKO")).unwrap(),
        )],
    );
    assert!(run(&no_drill).contains("no plated through-hole drill file"));

    let bad = dir.path().join("bad");
    std::fs::create_dir(&bad).unwrap();
    write_package(&bad, (0.0, 0.0, 40.0, 30.0), &[(10.0, 10.0)], &[]);
    std::fs::write(
        bad.join("Drill_PTH_Through.DRL"),
        "M48\nMETRIC\nT1C0.9\n%\nT1\nX--5\n",
    )
    .unwrap();
    let e = run(&bad);
    assert!(
        e.contains("Drill_PTH_Through.DRL:6") && e.contains("bad coordinate"),
        "{e}"
    );

    std::fs::write(
        bad.join("Drill_PTH_Through.DRL"),
        "M48\nMETRIC\nT1C0.9\n%\nT1\nX100.0Y100.0\n",
    )
    .unwrap();
    assert!(run(&bad).contains("outside the board outline"));

    std::fs::write(
        bad.join("Gerber_BoardOutlineLayer.GKO"),
        "%FSLAX33Y33*%\n%MOMM*%\nX1",
    )
    .unwrap();
    assert!(run(&bad).contains("Gerber_BoardOutlineLayer.GKO"));

    write_package(&bad, (0.0, 0.0, 40.0, 30.0), &[(10.0, 10.0)], &[]);
    std::fs::write(
        bad.join("PickAndPlace_bad.csv"),
        "Designator,Mid X,Mid Y\nR1,oops,2\n",
    )
    .unwrap();
    assert!(run(&bad).contains("bad Mid X"));
}

// ---------------------------------------------------------------- CLI binary

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ict-jig-gen"))
}

#[test]
fn cli_generate_writes_all_three_outputs() {
    let out = tempfile::tempdir().unwrap();
    let o = cli()
        .args(["generate"])
        .arg(demo_zip())
        .arg("--pnp")
        .arg(demo_pnp())
        .args([
            "--rail",
            "VCC-GND=4,5",
            "--image-size",
            "600x400",
            "--clearance",
            "0.2",
            "-o",
        ])
        .arg(out.path())
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8(o.stdout).unwrap();
    assert!(
        stdout.contains("Wire") && stdout.contains("R1.1") && stdout.contains("VCC-GND"),
        "{stdout}"
    );
    for f in ["cassette.stl", "lista_elementow.json", "wiring_guide.txt"] {
        assert!(
            std::fs::metadata(out.path().join(f)).unwrap().len() > 0,
            "{f}"
        );
    }
    let v = json_of(&out);
    assert_eq!(v["elementy"][3]["nazwa"], "R1");
    assert_eq!(v["elementy"][3]["x"], 150); // 15/60 * 600
    let mesh = common::read_stl(&out.path().join("cassette.stl"));
    assert!(mesh.topology().is_watertight());
}

#[test]
fn cli_reads_a_toml_config_and_flags_override_it() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("jig.toml");
    std::fs::write(
        &cfg,
        "pogo = \"P100-E2\"\nimage_size = \"800x800\"\nalignment = \"posts\"\n\n[[probe]]\npin = 0\nat = \"R1.1\"\n\n[[probe]]\npin = 1\nat = \"R1.2\"\n\n[[element]]\nname = \"VCC-GND\"\npins = [0, 1]\ntyp = \"linia\"\n",
    )
    .unwrap();
    let out = dir.path().join("o");
    let o = cli()
        .arg("generate")
        .arg(demo_zip())
        .arg("--pnp")
        .arg(demo_pnp())
        .arg("--config")
        .arg(&cfg)
        .args(["--pogo", "P75-LM2", "-o"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: Value =
        serde_json::from_slice(&std::fs::read(out.join("lista_elementow.json")).unwrap()).unwrap();
    assert_eq!(v["elementy"].as_array().unwrap().len(), 2); // derived R1 + the VCC-GND rail
    assert_eq!(v["elementy"][1]["typ"], "linia");
    let guide = std::fs::read_to_string(out.join("wiring_guide.txt")).unwrap();
    assert!(
        guide.contains("P75-LM2"),
        "CLI --pogo overrides the file: {guide}"
    );
    assert!(guide.contains("Channels in use: 2 of 16"));
}

#[test]
fn cli_holes_lists_candidates_and_failures_exit_non_zero() {
    let o = cli()
        .arg("holes")
        .arg(demo_zip())
        .arg("--pnp")
        .arg(demo_pnp())
        .output()
        .unwrap();
    assert!(o.status.success());
    let s = String::from_utf8(o.stdout).unwrap();
    assert!(
        s.contains("H13") && s.contains("U1.3") && s.contains("Mounting holes"),
        "{s}"
    );

    let o = cli()
        .arg("generate")
        .arg("/no/such/input.zip")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("error: "));

    let o = cli()
        .arg("generate")
        .arg(demo_zip())
        .args(["--probe", "99=H1"])
        .output()
        .unwrap();
    assert!(!o.status.success());
    let o = cli()
        .arg("generate")
        .arg(demo_zip())
        .args(["--image-size", "oops"])
        .output()
        .unwrap();
    assert!(!o.status.success());
}

#[test]
fn stray_text_and_lookalike_names_do_not_break_a_package() {
    let dir = tempfile::tempdir().unwrap();
    write_package(
        dir.path(),
        (0.0, 0.0, 40.0, 30.0),
        &[(10.0, 10.0), (14.0, 12.0)],
        &[],
    );
    std::fs::write(
        dir.path().join("drill_notes.txt"),
        "Call the fab about the T1 drill size\n",
    )
    .unwrap();
    // a plated drill whose name merely contains the letters "via" is still a plated drill
    std::fs::rename(
        dir.path().join("Drill_PTH_Through.DRL"),
        dir.path().join("Olivia_PTH_Through.DRL"),
    )
    .unwrap();
    let out = app::generate(&request(
        dir.path(),
        &dir.path().join("o"),
        ConfigFile::default(),
    ))
    .unwrap();
    assert_eq!(out.board.plated.len(), 2);
}

#[test]
fn pick_and_place_with_unannotated_parts_only_warns() {
    let dir = tempfile::tempdir().unwrap();
    write_package(
        dir.path(),
        (0.0, 0.0, 40.0, 30.0),
        &[(10.0, 10.0), (14.0, 12.0)],
        &[],
    );
    std::fs::write(
        dir.path().join("PickAndPlace_x.csv"),
        "Designator,Mid X,Mid Y\nR?,10,10\nR?,14,12\n",
    )
    .unwrap();
    let out = app::generate(&request(
        dir.path(),
        &dir.path().join("o"),
        ConfigFile::default(),
    ))
    .unwrap();
    assert!(
        out.warnings
            .iter()
            .any(|w| w.contains("duplicate designator R?")),
        "{:?}",
        out.warnings
    );
}

#[test]
fn example_config_file_loads_and_runs_on_the_fixture() {
    let cfg = ConfigFile::load(&common::fixtures().join("../../examples/jig.toml")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let out = app::generate(&GenerateRequest {
        pnp: Some(demo_pnp()),
        ..request(&demo_zip(), dir.path(), cfg)
    })
    .unwrap();
    assert_eq!(out.pin_map.probes.len(), 4);
    assert!(
        out.lista
            .elementy
            .iter()
            .any(|e| e.nazwa == "VCC-GND" && e.piny == [14, 15])
    );
}

#[test]
fn cli_split_flag_and_config_key_write_two_halves() {
    let dir = tempfile::tempdir().unwrap();
    let run = |extra: &[&str], out: &std::path::Path| {
        let o = cli()
            .arg("generate")
            .arg(demo_zip())
            .arg("--pnp")
            .arg(demo_pnp())
            .args(extra)
            .arg("-o")
            .arg(out)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap()
    };
    // flag
    let by_flag = dir.path().join("flag");
    let stdout = run(&["--split"], &by_flag);
    assert!(stdout.contains("cassette_bottom.stl") && stdout.contains("cassette_top.stl"));
    assert!(!by_flag.join("cassette.stl").exists());
    // config key
    let cfg = dir.path().join("jig.toml");
    std::fs::write(&cfg, "split = true\n").unwrap();
    let by_cfg = dir.path().join("cfg");
    run(&["--config", cfg.to_str().unwrap()], &by_cfg);
    for half in ["cassette_bottom.stl", "cassette_top.stl"] {
        assert!(
            common::read_stl(&by_cfg.join(half))
                .topology()
                .is_watertight()
        );
    }
    assert!(!by_cfg.join("cassette.stl").exists());
    // default
    let plain = dir.path().join("plain");
    run(&[], &plain);
    assert!(plain.join("cassette.stl").exists());
    assert!(!plain.join("cassette_top.stl").exists());
}
