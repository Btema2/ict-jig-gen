//! Command-line front end for `ict-jig-gen`.

// Doc comments on the clap structs are the `--help` text, so they are plain prose, not markdown.
#![allow(clippy::doc_markdown)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use ict_jig_gen::app::{self, GenerateRequest};
use ict_jig_gen::config::{Alignment, ConfigFile, ElementSpec, ImageSize, ProbeSpec, Typ};

/// Generate a pogo-pin test cassette (STL) and pin map (JSON) from an EasyEDA Gerber package.
#[derive(Debug, Parser)]
#[command(name = "ict-jig-gen", version, about, propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Write cassette.stl (or the --split halves), lista_elementow.json and wiring_guide.txt.
    Generate(Box<GenerateArgs>),
    /// List the plated holes (with H-ids) that can be assigned to multiplexer pins.
    Holes(HolesArgs),
}

#[derive(Debug, Args)]
struct HolesArgs {
    /// EasyEDA Gerber `.zip` or an unzipped directory.
    input: PathBuf,
    /// Pick & Place CSV (default: the one in the package, if any).
    #[arg(long, value_name = "CSV")]
    pnp: Option<PathBuf>,
    /// Also list via drill files (`*Via*.DRL`).
    #[arg(long)]
    include_vias: bool,
    /// Settings file; only `include_vias` matters here, but passing the same file as to
    /// `generate` guarantees both commands number the holes identically.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
struct GenerateArgs {
    /// EasyEDA Gerber `.zip` or an unzipped directory.
    input: PathBuf,
    /// Directory for the generated files.
    #[arg(short, long, default_value = "out", value_name = "DIR")]
    out_dir: PathBuf,
    /// TOML or JSON settings file; command-line options override it.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
    /// Pick & Place CSV (default: the one in the package, if any).
    #[arg(long, value_name = "CSV")]
    pnp: Option<PathBuf>,

    /// Assign a pin to a hole: PIN=REF with REF = H3 | R1.2 | X,Y (mm). Repeatable. When given,
    /// automatic selection is off.
    #[arg(long = "probe", value_name = "PIN=REF")]
    probes: Vec<ProbeSpec>,
    /// Add an element: NAME=A,B (pins). Repeatable.
    #[arg(long = "element", value_name = "NAME=A,B", value_parser = parse_element)]
    elements: Vec<ElementSpec>,
    /// Add a power rail (`linia`): NAME=A,B, e.g. VCC-GND=0,15. Repeatable.
    #[arg(long = "rail", value_name = "NAME=A,B", value_parser = parse_rail)]
    rails: Vec<ElementSpec>,
    /// Do not derive elements from components that have exactly two probed pads.
    #[arg(long)]
    no_auto_elements: bool,
    /// Also treat via drill files (`*Via*.DRL`) as probe candidates.
    #[arg(long)]
    include_vias: bool,

    /// Pogo model preset: P75-LM2 (default) or P100-E2.
    #[arg(long, value_name = "MODEL")]
    pogo: Option<String>,
    /// Pogo barrel diameter in mm (overrides the preset).
    #[arg(long, value_name = "MM")]
    barrel_diameter: Option<f64>,
    /// Pogo total length in mm (overrides the preset).
    #[arg(long, value_name = "MM")]
    pogo_length: Option<f64>,
    /// Pogo working stroke in mm (overrides the preset).
    #[arg(long, value_name = "MM")]
    pogo_stroke: Option<f64>,
    /// Extra hole diameter for FDM tolerance, in mm [default 0.15].
    #[arg(long, value_name = "MM")]
    clearance: Option<f64>,
    /// Thickness of each guide plate in mm [default 3].
    #[arg(long, value_name = "MM")]
    plate_thickness: Option<f64>,
    /// Gap between the two plates in mm [default 8].
    #[arg(long, value_name = "MM")]
    standoff_height: Option<f64>,
    /// Plate margin around the board outline in mm [default 8].
    #[arg(long, value_name = "MM")]
    margin: Option<f64>,
    /// Minimum printable wall between features in mm [default 0.8].
    #[arg(long, value_name = "MM")]
    min_wall: Option<f64>,
    /// Thickness of the tested PCB in mm [default 1.6].
    #[arg(long, value_name = "MM")]
    pcb_thickness: Option<f64>,
    /// Height of the board registration lip in mm [default 1].
    #[arg(long, value_name = "MM")]
    lip_height: Option<f64>,
    /// How the board is located: auto, posts, lip, both or none.
    #[arg(long, value_name = "MODE")]
    alignment: Option<Alignment>,
    /// Write two flat-printable halves (cassette_bottom.stl, cassette_top.stl) joined by four M3
    /// screws, so no FDM supports are needed, instead of one cassette.stl.
    #[arg(long)]
    split: bool,

    /// Take the pixel size from an existing board image (PNG/JPEG).
    #[arg(long, value_name = "FILE")]
    image: Option<PathBuf>,
    /// Pixel size of the board image as WxH [default 512x512]; wins over --image.
    #[arg(long, value_name = "WxH")]
    image_size: Option<ImageSize>,
}

fn parse_element(s: &str) -> Result<ElementSpec, String> {
    ElementSpec::parse_pair(s, Typ::Element)
}

fn parse_rail(s: &str) -> Result<ElementSpec, String> {
    ElementSpec::parse_pair(s, Typ::Linia)
}

impl GenerateArgs {
    /// The command-line values as a config overlay.
    fn overlay(&self) -> ConfigFile {
        ConfigFile {
            pogo: self.pogo.clone(),
            barrel_diameter: self.barrel_diameter,
            pogo_length: self.pogo_length,
            pogo_stroke: self.pogo_stroke,
            clearance: self.clearance,
            plate_thickness: self.plate_thickness,
            standoff_height: self.standoff_height,
            margin: self.margin,
            min_wall: self.min_wall,
            pcb_thickness: self.pcb_thickness,
            lip_height: self.lip_height,
            alignment: self.alignment,
            split: self.split.then_some(true),
            image: self.image.clone(),
            image_size: self.image_size.map(|s| format!("{}x{}", s.width, s.height)),
            include_vias: self.include_vias.then_some(true),
            auto_elements: self.no_auto_elements.then_some(false),
            probe: self.probes.clone(),
            element: self.elements.iter().chain(&self.rails).cloned().collect(),
        }
    }
}

fn run(cli: Cli) -> ict_jig_gen::Result<()> {
    match cli.command {
        Command::Holes(args) => {
            let from_config = match &args.config {
                Some(path) => ConfigFile::load(path)?.include_vias.unwrap_or(false),
                None => false,
            };
            print!(
                "{}",
                app::list_holes(
                    &args.input,
                    args.pnp.as_deref(),
                    args.include_vias || from_config
                )?
            );
        }
        Command::Generate(args) => {
            let mut config = match &args.config {
                Some(path) => ConfigFile::load(path)?,
                None => ConfigFile::default(),
            };
            config.overlay(args.overlay());
            let out = app::generate(&GenerateRequest {
                input: args.input,
                out_dir: args.out_dir,
                pnp: args.pnp,
                config,
            })?;
            print!("{}", out.guide);
            let stls: Vec<String> = out
                .stl_paths
                .iter()
                .zip(out.cassette.solids.meshes())
                .map(|(path, mesh)| {
                    format!("{} ({} triangles)", path.display(), mesh.triangle_count())
                })
                .collect();
            println!(
                "\nWrote {}, {} ({} entries), {}",
                stls.join(", "),
                out.json_path.display(),
                out.lista.elementy.len(),
                out.guide_path.display()
            );
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
