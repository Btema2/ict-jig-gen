//! User-facing settings: pogo-pin preset, mechanical tolerances, image size and the optional
//! explicit probe / element selection. Settings come from an optional TOML (or JSON) file
//! overlaid with command-line values; the file schema is [`ConfigFile`].

use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Highest multiplexer channel number (the fixture has 16 channels: `0..=15`).
pub const MAX_PIN: u8 = 15;

/// Kind of a `lista_elementow.json` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Typ {
    /// A component between two probes (`R1`, `D1`).
    #[default]
    Element,
    /// A supply rail between two probes (`VCC-GND`).
    Linia,
}

/// Dimensions of a spring probe (pogo pin), millimetres.
#[derive(Debug, Clone, PartialEq)]
pub struct PogoSpec {
    /// Model name, e.g. `P75-LM2`.
    pub name: String,
    /// Barrel (outer) diameter.
    pub barrel_diameter: f64,
    /// Total length, tail to plunger tip, uncompressed.
    pub length: f64,
    /// Working stroke of the plunger.
    pub stroke: f64,
}

/// Known pogo-pin models: `(name, barrel diameter, total length, stroke)`.
const PRESETS: &[(&str, f64, f64, f64)] = &[
    ("P75-LM2", 1.02, 16.54, 2.0),
    ("P100-E2", 1.36, 33.35, 6.35),
];

impl PogoSpec {
    /// Look up a preset by (case-insensitive) model name.
    pub fn preset(name: &str) -> Option<Self> {
        PRESETS
            .iter()
            .find(|p| p.0.eq_ignore_ascii_case(name))
            .map(|&(n, d, l, s)| Self {
                name: n.to_owned(),
                barrel_diameter: d,
                length: l,
                stroke: s,
            })
    }

    /// Names of the built-in presets.
    pub fn preset_names() -> Vec<&'static str> {
        PRESETS.iter().map(|p| p.0).collect()
    }
}

/// How the tested board is located on the cassette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Alignment {
    /// Posts when the board has two or more usable mounting holes, otherwise a corner lip.
    #[default]
    Auto,
    /// Posts through the board's mounting holes only.
    Posts,
    /// Raised registration lip around the board outline only.
    Lip,
    /// Both posts and lip.
    Both,
    /// No alignment features (plates only).
    None,
}

impl FromStr for Alignment {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "posts" => Ok(Self::Posts),
            "lip" => Ok(Self::Lip),
            "both" => Ok(Self::Both),
            "none" => Ok(Self::None),
            _ => Err(format!(
                "unknown alignment {s:?} (use auto, posts, lip, both or none)"
            )),
        }
    }
}

/// Mechanical parameters of the cassette, all in millimetres.
#[derive(Debug, Clone, PartialEq)]
pub struct Mechanical {
    /// Probe used in every channel.
    pub pogo: PogoSpec,
    /// Extra diameter added to pogo holes (and removed from alignment posts) for FDM tolerance.
    pub clearance: f64,
    /// Thickness of each guide plate.
    pub plate_thickness: f64,
    /// Gap between the two plates.
    pub standoff_height: f64,
    /// Plate overhang beyond the board outline on every side.
    pub margin: f64,
    /// Minimum printable wall between two features.
    pub min_wall: f64,
    /// Thickness of the tested PCB (sets alignment-post height).
    pub pcb_thickness: f64,
    /// Height of the registration lip.
    pub lip_height: f64,
    /// Alignment strategy.
    pub alignment: Alignment,
    /// Polygon segments per circle.
    pub circle_segments: usize,
    /// Emit two flat-printable halves joined by M3 screws instead of one combined solid.
    pub split: bool,
}

impl Default for Mechanical {
    fn default() -> Self {
        Self {
            pogo: PogoSpec::preset("P75-LM2").unwrap_or(PogoSpec {
                name: "P75-LM2".into(),
                barrel_diameter: 1.02,
                length: 16.54,
                stroke: 2.0,
            }),
            clearance: 0.15,
            plate_thickness: 3.0,
            standoff_height: 8.0,
            margin: 8.0,
            min_wall: 0.8,
            pcb_thickness: 1.6,
            lip_height: 1.0,
            alignment: Alignment::Auto,
            circle_segments: 32,
            split: false,
        }
    }
}

impl Mechanical {
    /// Diameter of the holes drilled for the pogo barrels.
    pub fn hole_diameter(&self) -> f64 {
        self.pogo.barrel_diameter + self.clearance
    }

    fn validate(&self) -> Result<()> {
        let check = |name: &str, v: f64, lo: f64, hi: f64| -> Result<()> {
            if v.is_finite() && (lo..=hi).contains(&v) {
                Ok(())
            } else {
                Err(Error::Config(format!(
                    "{name} = {v} is outside the supported range {lo}..={hi} mm"
                )))
            }
        };
        check("barrel diameter", self.pogo.barrel_diameter, 0.2, 10.0)?;
        check("pogo length", self.pogo.length, 2.0, 100.0)?;
        check("pogo stroke", self.pogo.stroke, 0.1, 50.0)?;
        check("clearance", self.clearance, 0.0, 1.0)?;
        check("plate thickness", self.plate_thickness, 1.0, 20.0)?;
        check("standoff height", self.standoff_height, 1.0, 100.0)?;
        check("margin", self.margin, 7.0, 50.0)?;
        check("min wall", self.min_wall, 0.2, 5.0)?;
        check("pcb thickness", self.pcb_thickness, 0.2, 6.0)?;
        check("lip height", self.lip_height, 0.2, 10.0)?;
        if !(8..=128).contains(&self.circle_segments) {
            return Err(Error::Config("circle segments must be 8..=128".into()));
        }
        Ok(())
    }
}

/// Pixel dimensions of the top-down board image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSize {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Default for ImageSize {
    fn default() -> Self {
        Self {
            width: 512,
            height: 512,
        }
    }
}

impl FromStr for ImageSize {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let bad = || format!("image size {s:?} must look like 512x512");
        let (w, h) = s.split_once(['x', 'X']).ok_or_else(bad)?;
        let (width, height) = (
            w.trim().parse::<u32>().map_err(|_| bad())?,
            h.trim().parse::<u32>().map_err(|_| bad())?,
        );
        if !(1..=65_535).contains(&width) || !(1..=65_535).contains(&height) {
            return Err(format!(
                "image size {s:?}: each side must be 1..=65535 pixels"
            ));
        }
        Ok(Self { width, height })
    }
}

/// Where the pixel dimensions come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSource {
    /// Explicit `WxH`.
    Size(ImageSize),
    /// Read from an existing PNG/JPEG.
    File(PathBuf),
}

impl ImageSource {
    /// Resolve to concrete pixel dimensions (reads the file header for [`Self::File`]).
    pub fn resolve(&self) -> Result<ImageSize> {
        match self {
            Self::Size(size) => Ok(*size),
            Self::File(path) => {
                let dims = imagesize::size(path).map_err(|e| Error::Image {
                    path: path.clone(),
                    message: e.to_string(),
                })?;
                let to_u32 = |v: usize| {
                    u32::try_from(v)
                        .ok()
                        .filter(|v| *v >= 1)
                        .ok_or_else(|| Error::Image {
                            path: path.clone(),
                            message: "image dimensions out of range".into(),
                        })
                };
                Ok(ImageSize {
                    width: to_u32(dims.width)?,
                    height: to_u32(dims.height)?,
                })
            }
        }
    }
}

/// Reference to a plated hole in a probe or element definition.
///
/// A string is a hole id (`H3`, as listed by the `holes` command) or a component pad
/// (`R1.2` = second pad of `R1`); a two-number array is a board position in mm that must match
/// a hole to within 0.5 mm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PadRef {
    /// `H3` or `R1.2`.
    Name(String),
    /// `[x, y]` in board millimetres.
    Xy([f64; 2]),
}

impl FromStr for PadRef {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let s = s.trim();
        if let Some((x, y)) = s.split_once(',') {
            let n = |v: &str| v.trim().parse::<f64>().ok().filter(|v| v.is_finite());
            return match (n(x), n(y)) {
                (Some(x), Some(y)) => Ok(Self::Xy([x, y])),
                _ => Err(format!("bad coordinate pair {s:?}; expected X,Y in mm")),
            };
        }
        if s.is_empty() {
            return Err("empty pad reference".into());
        }
        Ok(Self::Name(s.to_owned()))
    }
}

/// Assign a multiplexer pin to a hole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeSpec {
    /// Multiplexer channel, `0..=15`.
    pub pin: u8,
    /// Which hole.
    pub at: PadRef,
    /// Optional label for the wiring guide.
    #[serde(default)]
    pub label: Option<String>,
}

impl FromStr for ProbeSpec {
    type Err = String;
    /// `PIN=REF`, e.g. `0=R1.1`, `5=H12`, `7=112.5,80`.
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let (pin, at) = s
            .split_once('=')
            .ok_or_else(|| format!("{s:?}: expected PIN=REF"))?;
        let pin = pin
            .trim()
            .parse::<u8>()
            .map_err(|_| format!("{s:?}: pin must be 0..=15"))?;
        Ok(Self {
            pin,
            at: at.parse()?,
            label: None,
        })
    }
}

/// An entry of `lista_elementow.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementSpec {
    /// Display name (`R1`, `VCC-GND`).
    pub name: String,
    /// The two multiplexer pins being measured.
    pub pins: [u8; 2],
    /// `element` (default) or `linia`.
    #[serde(default)]
    pub typ: Typ,
    /// Position on the board image in board mm; defaults to the component centroid or the
    /// midpoint of the two probes.
    #[serde(default)]
    pub at: Option<[f64; 2]>,
}

impl ElementSpec {
    /// Parse `NAME=A,B` with the given kind.
    pub fn parse_pair(s: &str, typ: Typ) -> std::result::Result<Self, String> {
        let (name, pins) = s
            .split_once('=')
            .ok_or_else(|| format!("{s:?}: expected NAME=A,B"))?;
        let (a, b) = pins
            .split_once(',')
            .ok_or_else(|| format!("{s:?}: expected NAME=A,B"))?;
        let pin = |v: &str| {
            v.trim()
                .parse::<u8>()
                .map_err(|_| format!("{s:?}: pins must be 0..=15"))
        };
        Ok(Self {
            name: name.trim().to_owned(),
            pins: [pin(a)?, pin(b)?],
            typ,
            at: None,
        })
    }
}

/// Schema of the configuration file (TOML or JSON). Every key is optional.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    /// Pogo preset name (`P75-LM2`, `P100-E2`) or a custom model name.
    pub pogo: Option<String>,
    /// Barrel diameter override.
    pub barrel_diameter: Option<f64>,
    /// Total pogo length override.
    pub pogo_length: Option<f64>,
    /// Pogo stroke override.
    pub pogo_stroke: Option<f64>,
    /// Hole clearance for FDM printing.
    pub clearance: Option<f64>,
    /// Guide plate thickness.
    pub plate_thickness: Option<f64>,
    /// Gap between the plates.
    pub standoff_height: Option<f64>,
    /// Plate margin around the board.
    pub margin: Option<f64>,
    /// Minimum printable wall.
    pub min_wall: Option<f64>,
    /// PCB thickness.
    pub pcb_thickness: Option<f64>,
    /// Registration lip height.
    pub lip_height: Option<f64>,
    /// Alignment strategy.
    pub alignment: Option<Alignment>,
    /// Write `cassette_bottom.stl` + `cassette_top.stl` (print flat, no supports) instead of
    /// one `cassette.stl`.
    pub split: Option<bool>,
    /// Existing board image to take pixel dimensions from.
    pub image: Option<PathBuf>,
    /// Explicit `WxH` (wins over `image`).
    pub image_size: Option<String>,
    /// Include `*Via*.DRL` files as probe candidates.
    pub include_vias: Option<bool>,
    /// Derive `element` entries from components with exactly two probed pads (default true).
    pub auto_elements: Option<bool>,
    /// Explicit pin assignments; when present they replace automatic selection.
    #[serde(default)]
    pub probe: Vec<ProbeSpec>,
    /// Explicit element / rail definitions.
    #[serde(default)]
    pub element: Vec<ElementSpec>,
}

impl ConfigFile {
    /// Read a `.toml` or `.json` config file.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        let is_json = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"));
        let parsed = if is_json {
            serde_json::from_str(&text).map_err(|e| e.to_string())
        } else {
            toml::from_str(&text).map_err(|e| e.to_string())
        };
        parsed.map_err(|e| Error::Config(format!("{}: {e}", path.display())))
    }

    /// Apply `over` on top of `self`: set values win, probes replace by pin, elements by name.
    pub fn overlay(&mut self, over: Self) {
        macro_rules! take {
            ($($f:ident),*) => { $( if over.$f.is_some() { self.$f = over.$f; } )* };
        }
        take!(
            pogo,
            barrel_diameter,
            pogo_length,
            pogo_stroke,
            clearance,
            plate_thickness,
            standoff_height,
            margin,
            min_wall,
            pcb_thickness,
            lip_height,
            alignment,
            split,
            include_vias,
            auto_elements
        );
        if over.image.is_some() || over.image_size.is_some() {
            self.image = over.image;
            self.image_size = over.image_size;
        }
        for p in over.probe {
            self.probe.retain(|q| q.pin != p.pin);
            self.probe.push(p);
        }
        for e in over.element {
            self.element.retain(|q| q.name != e.name);
            self.element.push(e);
        }
    }
}

/// Fully resolved and validated settings.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Cassette mechanics.
    pub mech: Mechanical,
    /// Pixel size source.
    pub image: ImageSource,
    /// Whether via drill files count as probe candidates.
    pub include_vias: bool,
    /// Whether to derive elements automatically.
    pub auto_elements: bool,
    /// Explicit probes (empty = automatic).
    pub probes: Vec<ProbeSpec>,
    /// Explicit elements.
    pub elements: Vec<ElementSpec>,
}

impl Settings {
    /// Resolve defaults + file + overrides into validated settings.
    pub fn resolve(cfg: ConfigFile) -> Result<Self> {
        let mut mech = Mechanical::default();
        let mut custom = false;
        if let Some(name) = &cfg.pogo {
            mech.pogo = if let Some(preset) = PogoSpec::preset(name) {
                preset
            } else {
                let (Some(barrel_diameter), Some(length), Some(stroke)) =
                    (cfg.barrel_diameter, cfg.pogo_length, cfg.pogo_stroke)
                else {
                    return Err(Error::Config(format!(
                        "unknown pogo model {name:?}; known: {}. A custom model needs barrel_diameter, \
                         pogo_length and pogo_stroke",
                        PogoSpec::preset_names().join(", ")
                    )));
                };
                custom = true;
                PogoSpec {
                    name: name.clone(),
                    barrel_diameter,
                    length,
                    stroke,
                }
            };
        }
        let set = |slot: &mut f64, v: Option<f64>| {
            if let Some(v) = v {
                *slot = v;
            }
        };
        set(&mut mech.pogo.barrel_diameter, cfg.barrel_diameter);
        set(&mut mech.pogo.length, cfg.pogo_length);
        set(&mut mech.pogo.stroke, cfg.pogo_stroke);
        if !custom
            && (cfg.barrel_diameter.is_some()
                || cfg.pogo_length.is_some()
                || cfg.pogo_stroke.is_some())
        {
            mech.pogo.name = format!("{} (modified)", mech.pogo.name);
        }
        set(&mut mech.clearance, cfg.clearance);
        set(&mut mech.plate_thickness, cfg.plate_thickness);
        set(&mut mech.standoff_height, cfg.standoff_height);
        set(&mut mech.margin, cfg.margin);
        set(&mut mech.min_wall, cfg.min_wall);
        set(&mut mech.pcb_thickness, cfg.pcb_thickness);
        set(&mut mech.lip_height, cfg.lip_height);
        if let Some(a) = cfg.alignment {
            mech.alignment = a;
        }
        mech.split = cfg.split.unwrap_or(false);
        mech.validate()?;

        let image = match (&cfg.image_size, &cfg.image) {
            (Some(s), _) => ImageSource::Size(s.parse().map_err(Error::Config)?),
            (None, Some(p)) => ImageSource::File(p.clone()),
            (None, None) => ImageSource::Size(ImageSize::default()),
        };
        for pin in cfg
            .probe
            .iter()
            .map(|p| p.pin)
            .chain(cfg.element.iter().flat_map(|e| e.pins))
        {
            if pin > MAX_PIN {
                return Err(Error::Config(format!(
                    "pin {pin} is out of range 0..={MAX_PIN}"
                )));
            }
        }
        Ok(Self {
            mech,
            image,
            include_vias: cfg.include_vias.unwrap_or(false),
            auto_elements: cfg.auto_elements.unwrap_or(true),
            probes: cfg.probe,
            elements: cfg.element,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_p75_lm2() {
        let s = Settings::resolve(ConfigFile::default()).unwrap();
        assert_eq!(s.mech.pogo.name, "P75-LM2");
        assert!((s.mech.pogo.barrel_diameter - 1.02).abs() < 1e-12);
        assert!((s.mech.pogo.length - 16.54).abs() < 1e-12);
        assert!((s.mech.hole_diameter() - 1.17).abs() < 1e-12);
        assert_eq!(
            s.image,
            ImageSource::Size(ImageSize {
                width: 512,
                height: 512
            })
        );
    }

    #[test]
    fn switching_pogo_is_one_setting() {
        let cfg = ConfigFile {
            pogo: Some("p100-e2".into()),
            ..ConfigFile::default()
        };
        let s = Settings::resolve(cfg).unwrap();
        assert_eq!(s.mech.pogo.name, "P100-E2");
        assert!((s.mech.pogo.barrel_diameter - 1.36).abs() < 1e-12);
    }

    #[test]
    fn custom_pogo_needs_every_dimension_and_overrides_are_labelled() {
        let cfg = ConfigFile {
            pogo: Some("XYZ".into()),
            barrel_diameter: Some(0.8),
            ..ConfigFile::default()
        };
        let e = Settings::resolve(cfg).unwrap_err().to_string();
        assert!(
            e.contains("known: P75-LM2") && e.contains("pogo_length"),
            "{e}"
        );
        let cfg = ConfigFile {
            pogo: Some("XYZ".into()),
            barrel_diameter: Some(0.8),
            pogo_length: Some(20.0),
            pogo_stroke: Some(3.0),
            ..ConfigFile::default()
        };
        let pogo = Settings::resolve(cfg).unwrap().mech.pogo;
        assert_eq!(
            (pogo.name.as_str(), pogo.length, pogo.stroke),
            ("XYZ", 20.0, 3.0)
        );
        // overriding one dimension of a preset must not leave the preset's name on a different probe
        let cfg = ConfigFile {
            barrel_diameter: Some(1.1),
            ..ConfigFile::default()
        };
        assert_eq!(
            Settings::resolve(cfg).unwrap().mech.pogo.name,
            "P75-LM2 (modified)"
        );
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        for cfg in [
            ConfigFile {
                clearance: Some(-0.1),
                ..ConfigFile::default()
            },
            ConfigFile {
                plate_thickness: Some(f64::NAN),
                ..ConfigFile::default()
            },
            ConfigFile {
                margin: Some(2.0),
                ..ConfigFile::default()
            },
            ConfigFile {
                barrel_diameter: Some(0.0),
                ..ConfigFile::default()
            },
        ] {
            assert!(Settings::resolve(cfg).is_err());
        }
        let bad_pin = ConfigFile {
            element: vec![ElementSpec {
                name: "x".into(),
                pins: [0, 16],
                typ: Typ::Element,
                at: None,
            }],
            ..ConfigFile::default()
        };
        assert!(
            Settings::resolve(bad_pin)
                .unwrap_err()
                .to_string()
                .contains("pin 16")
        );
    }

    #[test]
    fn image_size_parsing() {
        assert_eq!(
            "640x480".parse::<ImageSize>().unwrap(),
            ImageSize {
                width: 640,
                height: 480
            }
        );
        assert_eq!("640X480".parse::<ImageSize>().unwrap().height, 480);
        for bad in ["640", "0x10", "axb", "70000x10", "-1x5", ""] {
            assert!(bad.parse::<ImageSize>().is_err(), "{bad}");
        }
    }

    #[test]
    fn toml_config_round_trip_and_unknown_keys() {
        let cfg: ConfigFile = toml::from_str(
            r#"
            pogo = "P100-E2"
            clearance = 0.2
            image_size = "800x600"
            [[probe]]
            pin = 0
            at = "R1.1"
            [[probe]]
            pin = 1
            at = [112.5, 80.0]
            [[element]]
            name = "VCC-GND"
            pins = [0, 15]
            typ = "linia"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.probe.len(), 2);
        assert_eq!(cfg.probe[1].at, PadRef::Xy([112.5, 80.0]));
        assert_eq!(cfg.element[0].typ, Typ::Linia);
        assert!(toml::from_str::<ConfigFile>("clearence = 1").is_err());
    }

    #[test]
    fn split_is_off_by_default_and_follows_file_then_flag() {
        assert!(!Settings::resolve(ConfigFile::default()).unwrap().mech.split);
        let mut cfg: ConfigFile = toml::from_str("split = true").unwrap();
        assert!(Settings::resolve(cfg.clone()).unwrap().mech.split);
        // an absent command-line value leaves the file's choice alone
        cfg.overlay(ConfigFile::default());
        assert_eq!(cfg.split, Some(true));
        cfg.overlay(ConfigFile {
            split: Some(false),
            ..ConfigFile::default()
        });
        assert!(!Settings::resolve(cfg).unwrap().mech.split);
    }

    #[test]
    fn overlay_replaces_by_pin_and_name() {
        let mut base = ConfigFile {
            clearance: Some(0.1),
            probe: vec!["0=R1.1".parse().unwrap(), "1=R1.2".parse().unwrap()],
            element: vec![ElementSpec::parse_pair("A=0,1", Typ::Element).unwrap()],
            ..ConfigFile::default()
        };
        base.overlay(ConfigFile {
            clearance: Some(0.3),
            probe: vec!["0=H9".parse().unwrap()],
            element: vec![ElementSpec::parse_pair("A=2,3", Typ::Linia).unwrap()],
            ..ConfigFile::default()
        });
        assert_eq!(base.clearance, Some(0.3));
        assert_eq!(base.probe.len(), 2);
        assert!(
            base.probe
                .iter()
                .any(|p| p.pin == 0 && p.at == PadRef::Name("H9".into()))
        );
        assert_eq!(base.element.len(), 1);
        assert_eq!(base.element[0].pins, [2, 3]);
    }

    #[test]
    fn cli_arg_parsers() {
        assert_eq!("3=H7".parse::<ProbeSpec>().unwrap().pin, 3);
        assert_eq!(
            "3=1.5, 2".parse::<ProbeSpec>().unwrap().at,
            PadRef::Xy([1.5, 2.0])
        );
        assert!("x=H7".parse::<ProbeSpec>().is_err());
        assert!("3".parse::<ProbeSpec>().is_err());
        assert!("3=1.5,zz".parse::<ProbeSpec>().is_err());
        let e = ElementSpec::parse_pair("5V-GND=3,15", Typ::Linia).unwrap();
        assert_eq!(
            (e.name.as_str(), e.pins, e.typ),
            ("5V-GND", [3, 15], Typ::Linia)
        );
        assert!(ElementSpec::parse_pair("R1=0", Typ::Element).is_err());
        assert!("bogus".parse::<Alignment>().is_err());
    }

    #[test]
    fn missing_config_file_is_an_io_error() {
        assert!(matches!(
            ConfigFile::load(Path::new("/no/such.toml")),
            Err(Error::Io { .. })
        ));
    }

    #[test]
    fn image_source_reads_png_header() {
        // 8-byte signature + IHDR for a 3x2 image
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&3u32.to_be_bytes());
        png.extend_from_slice(&2u32.to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        std::fs::write(&path, &png).unwrap();
        let size = ImageSource::File(path).resolve().unwrap();
        assert_eq!(
            size,
            ImageSize {
                width: 3,
                height: 2
            }
        );
        let junk = dir.path().join("junk.png");
        std::fs::write(&junk, b"not an image").unwrap();
        assert!(matches!(
            ImageSource::File(junk).resolve(),
            Err(Error::Image { .. })
        ));
    }
}
