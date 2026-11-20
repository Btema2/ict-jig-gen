//! Probe selection (which hole feeds which multiplexer pin), element derivation and the
//! `lista_elementow.json` export.

use std::cmp::Ordering;
use std::collections::HashMap;

use serde::Serialize;

use crate::board::Board;
use crate::config::{ElementSpec, MAX_PIN, PadRef, ProbeSpec, Settings};
pub use crate::config::{ImageSize, Typ};
use crate::error::{Error, Result};
use crate::excellon::Hole;
use crate::geom::{BBox, Point};

/// A hole is attributed to the nearest component centroid within this distance (mm).
const ASSOCIATION_RADIUS_MM: f64 = 15.0;
/// Tolerance when a probe is given as an `[x, y]` position.
const COORD_MATCH_TOLERANCE_MM: f64 = 0.5;

/// A plated hole together with its derived identity.
#[derive(Debug, Clone, PartialEq)]
pub struct HoleInfo {
    /// 1-based id in reading order (`H1`, `H2`, ...).
    pub id: usize,
    /// The hole.
    pub hole: Hole,
    /// Index into `Board::components` of the nearest component, if any.
    pub component: Option<usize>,
    /// 1-based pad number within that component (left to right, then top to bottom).
    pub pad: Option<usize>,
}

/// A hole wired to a multiplexer channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    /// Multiplexer channel `0..=15`.
    pub pin: u8,
    /// Position and drill of the pad (board mm).
    pub hole: Hole,
    /// Hole id (`H<id>`).
    pub hole_id: usize,
    /// Human label: `R1.2`, or `H7` when no component is known.
    pub label: String,
    /// Index into `Board::components`.
    pub component: Option<usize>,
}

/// One line of the app's element list.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// Display name.
    pub name: String,
    /// The two pins measured.
    pub pins: [u8; 2],
    /// Element or rail.
    pub typ: Typ,
    /// Position on the board (board mm), mapped to pixels on export.
    pub at: Point,
}

/// The complete pin assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct PinMap {
    /// Wired probes sorted by pin.
    pub probes: Vec<Probe>,
    /// Elements to export.
    pub elements: Vec<Element>,
    /// Non-fatal observations.
    pub warnings: Vec<String>,
}

/// Number the plated holes and attribute each to a component.
pub fn catalog(board: &Board) -> Vec<HoleInfo> {
    let mut infos: Vec<HoleInfo> = board
        .plated
        .iter()
        .enumerate()
        .map(|(i, &hole)| {
            let component = board
                .components
                .iter()
                .enumerate()
                .map(|(idx, c)| (idx, c.pos.distance(hole.pos)))
                .filter(|&(_, d)| d <= ASSOCIATION_RADIUS_MM)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(idx, _)| idx);
            HoleInfo {
                id: i + 1,
                hole,
                component,
                pad: None,
            }
        })
        .collect();
    let mut by_component: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, info) in infos.iter().enumerate() {
        if let Some(c) = info.component {
            by_component.entry(c).or_default().push(i);
        }
    }
    for members in by_component.values_mut() {
        members.sort_by(|&a, &b| {
            let (pa, pb) = (infos[a].hole.pos, infos[b].hole.pos);
            pa.x.total_cmp(&pb.x).then(pb.y.total_cmp(&pa.y))
        });
        for (n, &i) in members.iter().enumerate() {
            infos[i].pad = Some(n + 1);
        }
    }
    infos
}

fn label(board: &Board, info: &HoleInfo) -> String {
    match (info.component, info.pad) {
        (Some(c), Some(p)) => format!("{}.{p}", board.components[c].designator),
        _ => format!("H{}", info.id),
    }
}

/// `R10` -> ("R", 10): orders designators the way humans expect.
fn natural_key(designator: &str) -> (String, u64, String) {
    let split = designator
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(designator.len());
    let number = designator[split..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    (
        designator[..split].to_owned(),
        number,
        designator.to_owned(),
    )
}

fn resolve_ref(board: &Board, infos: &[HoleInfo], r: &PadRef) -> Result<usize> {
    match r {
        PadRef::Xy([x, y]) => {
            let target = Point::new(*x, *y);
            infos
                .iter()
                .enumerate()
                .map(|(i, h)| (i, h.hole.pos.distance(target)))
                .filter(|&(_, d)| d <= COORD_MATCH_TOLERANCE_MM)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
                .ok_or_else(|| {
                    Error::Selection(format!(
                        "no plated hole within {COORD_MATCH_TOLERANCE_MM} mm of ({x}, {y})"
                    ))
                })
        }
        PadRef::Name(name) => {
            let hole_id = name
                .strip_prefix(['H', 'h'])
                .filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|d| d.parse::<usize>().ok());
            if let Some(id) = hole_id {
                return infos.iter().position(|h| h.id == id).ok_or_else(|| {
                    Error::Selection(format!(
                        "hole {name} does not exist (board has {} holes)",
                        infos.len()
                    ))
                });
            }
            let pad = name
                .split_once('.')
                .and_then(|(d, p)| Some((d, p.parse::<usize>().ok()?)));
            let Some((designator, pad)) = pad else {
                return Err(Error::Selection(format!(
                    "cannot interpret {name:?}: use a hole id (H3), a pad (R1.2) or coordinates"
                )));
            };
            infos
                .iter()
                .position(|h| {
                    h.pad == Some(pad)
                        && h.component
                            .is_some_and(|c| board.components[c].designator == designator)
                })
                .ok_or_else(|| Error::Selection(format!("no plated hole matches pad {name}")))
        }
    }
}

impl PinMap {
    /// Decide the pin assignment and the element list for `board`.
    pub fn build(board: &Board, settings: &Settings) -> Result<Self> {
        let infos = catalog(board);
        let mut warnings = Vec::new();
        let assignment = if settings.probes.is_empty() {
            auto_assignment(board, &infos)?
        } else {
            explicit_assignment(board, &infos, &settings.probes)?
        };
        let mut probes: Vec<Probe> = assignment
            .into_iter()
            .map(|(pin, idx, custom)| {
                let info = &infos[idx];
                Probe {
                    pin,
                    hole: info.hole,
                    hole_id: info.id,
                    label: custom.unwrap_or_else(|| label(board, info)),
                    component: info.component,
                }
            })
            .collect();
        probes.sort_by_key(|p| p.pin);

        let mut elements = Vec::new();
        if settings.auto_elements {
            elements = derive_elements(board, &probes, &mut warnings);
        }
        for spec in &settings.elements {
            let element = explicit_element(board, &probes, spec)?;
            elements.retain(|e| e.name != element.name);
            elements.push(element);
        }
        if elements.is_empty() {
            warnings.push(
                "no elements defined: lista_elementow.json will be empty; add [[element]] entries \
                 or a Pick & Place file"
                    .into(),
            );
        }
        Ok(Self {
            probes,
            elements,
            warnings,
        })
    }

    /// Convert to the exchange structure, mapping board mm to pixels.
    pub fn to_lista(&self, bbox: &BBox, size: ImageSize) -> Result<ListaElementow> {
        let elementy = self
            .elements
            .iter()
            .map(|e| {
                let (x, y) = to_pixel(bbox, size, e.at);
                Wpis {
                    nazwa: e.name.clone(),
                    piny: e.pins,
                    x,
                    y,
                    typ: e.typ,
                }
            })
            .collect::<Vec<_>>();
        let lista = ListaElementow { elementy };
        lista.validate()?;
        Ok(lista)
    }
}

/// `(pin, catalog index, custom label)`
type Assignment = Vec<(u8, usize, Option<String>)>;

fn auto_assignment(board: &Board, infos: &[HoleInfo]) -> Result<Assignment> {
    let capacity = usize::from(MAX_PIN) + 1;
    if infos.len() > capacity {
        return Err(Error::Selection(format!(
            "the board has {} plated holes but the fixture has only {capacity} channels; choose \
             which ones to probe with --probe PIN=REF (e.g. --probe 0=R1.1 --probe 1=H12) or [[probe]] \
             entries in a config file. `ict-jig-gen holes <input>` lists the candidates",
            infos.len()
        )));
    }
    let mut order: Vec<usize> = (0..infos.len()).collect();
    order.sort_by(|&a, &b| {
        let key = |i: usize| {
            infos[i]
                .component
                .map(|c| natural_key(&board.components[c].designator))
        };
        match (key(a), key(b)) {
            (Some(ka), Some(kb)) => ka.cmp(&kb).then(infos[a].pad.cmp(&infos[b].pad)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => infos[a].id.cmp(&infos[b].id),
        }
    });
    Ok(order
        .into_iter()
        .zip(0..=MAX_PIN)
        .map(|(idx, pin)| (pin, idx, None))
        .collect())
}

fn explicit_assignment(
    board: &Board,
    infos: &[HoleInfo],
    probes: &[ProbeSpec],
) -> Result<Assignment> {
    let mut out: Assignment = Vec::new();
    for spec in probes {
        if spec.pin > MAX_PIN {
            return Err(Error::Selection(format!(
                "pin {} is out of range 0..={MAX_PIN}",
                spec.pin
            )));
        }
        let idx = resolve_ref(board, infos, &spec.at)?;
        if let Some(prev) = out.iter().find(|(p, _, _)| *p == spec.pin) {
            return Err(Error::Selection(format!(
                "pin {} is assigned twice (H{} and H{})",
                spec.pin, infos[prev.1].id, infos[idx].id
            )));
        }
        if let Some(prev) = out.iter().find(|(_, i, _)| *i == idx) {
            return Err(Error::Selection(format!(
                "hole H{} would feed both pin {} and pin {}",
                infos[idx].id, prev.0, spec.pin
            )));
        }
        out.push((spec.pin, idx, spec.label.clone()));
    }
    Ok(out)
}

fn derive_elements(board: &Board, probes: &[Probe], warnings: &mut Vec<String>) -> Vec<Element> {
    let mut by_component: HashMap<usize, Vec<&Probe>> = HashMap::new();
    for p in probes {
        if let Some(c) = p.component {
            by_component.entry(c).or_default().push(p);
        }
    }
    let mut elements = Vec::new();
    for (c, mut members) in by_component {
        let comp = &board.components[c];
        members.sort_by(|a, b| {
            a.hole
                .pos
                .x
                .total_cmp(&b.hole.pos.x)
                .then(b.hole.pos.y.total_cmp(&a.hole.pos.y))
        });
        match members.as_slice() {
            [a, b] => elements.push(Element {
                name: comp.designator.clone(),
                pins: [a.pin, b.pin],
                typ: Typ::Element,
                at: comp.pos,
            }),
            [_] => warnings.push(format!(
                "{}: only one pad is probed, no element generated",
                comp.designator
            )),
            many => warnings.push(format!(
                "{}: {} probed pads; a 2-pin element cannot be derived automatically. Define the \
                 pairs you want with --element NAME=A,B",
                comp.designator,
                many.len()
            )),
        }
    }
    elements.sort_by_key(|e| (e.pins[0].min(e.pins[1]), e.pins[0].max(e.pins[1])));
    elements
}

fn explicit_element(board: &Board, probes: &[Probe], spec: &ElementSpec) -> Result<Element> {
    if spec.name.trim().is_empty() {
        return Err(Error::Selection("element with an empty name".into()));
    }
    let [a, b] = spec.pins;
    if a == b {
        return Err(Error::Selection(format!(
            "element {}: both pins are {a}",
            spec.name
        )));
    }
    let find = |pin: u8| {
        probes.iter().find(|p| p.pin == pin).ok_or_else(|| {
            Error::Selection(format!(
                "element {} uses pin {pin}, which has no probe assigned",
                spec.name
            ))
        })
    };
    let (pa, pb) = (find(a)?, find(b)?);
    let at = match (spec.at, pa.component, pb.component) {
        (Some([x, y]), _, _) => Point::new(x, y),
        (None, Some(ca), Some(cb)) if ca == cb => board.components[ca].pos,
        _ => pa.hole.pos.midpoint(pb.hole.pos),
    };
    Ok(Element {
        name: spec.name.clone(),
        pins: spec.pins,
        typ: spec.typ,
        at,
    })
}

/// Map board millimetres to image pixels: linear over the outline bounding box, origin at the
/// image's top-left, Y flipped (Gerber +Y is up, image +Y is down). Results are clamped to the
/// image.
pub fn to_pixel(bbox: &BBox, size: ImageSize, p: Point) -> (u32, u32) {
    let axis = |fraction: f64, pixels: u32| -> u32 {
        let max = f64::from(pixels - 1);
        let value = (fraction * f64::from(pixels)).round().clamp(0.0, max);
        // clamped to 0..=pixels-1 above, so the cast cannot truncate or change sign
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = value as u32;
        v
    };
    let fx = (p.x - bbox.min.x) / bbox.width();
    let fy = (bbox.max.y - p.y) / bbox.height();
    (axis(fx, size.width), axis(fy, size.height))
}

/// One entry of `lista_elementow.json`; field order is the file's key order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Wpis {
    /// Name shown in the app.
    pub nazwa: String,
    /// The two multiplexer pins.
    pub piny: [u8; 2],
    /// Pixel X (0 = left).
    pub x: u32,
    /// Pixel Y (0 = top).
    pub y: u32,
    /// `element` or `linia`.
    pub typ: Typ,
}

/// The whole file: `{ "elementy": [...] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListaElementow {
    /// Entries in display order.
    pub elementy: Vec<Wpis>,
}

impl ListaElementow {
    /// Check the contract rules (pins distinct and in range, names non-empty).
    pub fn validate(&self) -> Result<()> {
        for e in &self.elementy {
            if e.nazwa.trim().is_empty() {
                return Err(Error::Selection("an element has an empty name".into()));
            }
            if e.piny[0] == e.piny[1] || e.piny.iter().any(|p| *p > MAX_PIN) {
                return Err(Error::Selection(format!(
                    "element {}: pins {:?} must be two distinct values in 0..={MAX_PIN}",
                    e.nazwa, e.piny
                )));
            }
        }
        Ok(())
    }

    /// Serialise: one entry per line, UTF-8 left as is (no `\u` escaping), trailing newline.
    pub fn to_json(&self) -> Result<String> {
        let mut out = String::from("{\n  \"elementy\": [");
        for (i, e) in self.elementy.iter().enumerate() {
            out.push_str(if i == 0 { "\n    " } else { ",\n    " });
            out.push_str(&serde_json::to_string(e)?);
        }
        out.push_str(if self.elementy.is_empty() {
            "]\n}\n"
        } else {
            "\n  ]\n}\n"
        });
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigFile;
    use crate::pnp::Component;

    fn hole(x: f64, y: f64) -> Hole {
        Hole {
            pos: Point::new(x, y),
            diameter: 0.8,
        }
    }

    fn comp(d: &str, x: f64, y: f64) -> Component {
        Component {
            designator: d.into(),
            pos: Point::new(x, y),
        }
    }

    fn board(holes: Vec<Hole>, components: Vec<Component>) -> Board {
        let mut b = Board {
            source: "t".into(),
            bbox: BBox {
                min: Point::new(0.0, 0.0),
                max: Point::new(100.0, 50.0),
            },
            plated: holes,
            mounting: vec![],
            components: vec![],
            warnings: vec![],
        };
        crate::board::sort_reading_order(&mut b.plated);
        b.components = components;
        b
    }

    fn settings(f: impl FnOnce(&mut ConfigFile)) -> Settings {
        let mut c = ConfigFile::default();
        f(&mut c);
        Settings::resolve(c).unwrap()
    }

    fn sample() -> Board {
        board(
            vec![
                hole(10.0, 40.0),
                hole(15.0, 40.0),
                hole(30.0, 20.0),
                hole(35.0, 20.0),
                hole(60.0, 10.0),
            ],
            vec![comp("R1", 12.5, 40.0), comp("D1", 32.5, 20.0)],
        )
    }

    #[test]
    fn pixel_mapping_flips_y_and_is_linear() {
        let bbox = BBox {
            min: Point::new(100.0, 50.0),
            max: Point::new(160.0, 90.0),
        };
        let size = ImageSize {
            width: 600,
            height: 400,
        };
        assert_eq!(to_pixel(&bbox, size, Point::new(100.0, 90.0)), (0, 0)); // top-left
        assert_eq!(to_pixel(&bbox, size, Point::new(160.0, 50.0)), (599, 399)); // bottom-right, clamped
        assert_eq!(to_pixel(&bbox, size, Point::new(130.0, 70.0)), (300, 200)); // centre
        assert_eq!(to_pixel(&bbox, size, Point::new(110.0, 80.0)), (100, 100));
        assert_eq!(to_pixel(&bbox, size, Point::new(500.0, -9.0)), (599, 399)); // far outside
    }

    #[test]
    fn auto_mode_assigns_by_designator_and_derives_elements() {
        let b = sample();
        let map = PinMap::build(&b, &settings(|_| {})).unwrap();
        assert_eq!(map.probes.len(), 5);
        let label = |pin: u8| {
            map.probes
                .iter()
                .find(|p| p.pin == pin)
                .unwrap()
                .label
                .clone()
        };
        assert_eq!((label(0).as_str(), label(1).as_str()), ("D1.1", "D1.2"));
        assert_eq!((label(2).as_str(), label(3).as_str()), ("R1.1", "R1.2"));
        assert!(label(4).starts_with('H'));
        let names: Vec<_> = map
            .elements
            .iter()
            .map(|e| (e.name.as_str(), e.pins))
            .collect();
        assert_eq!(names, [("D1", [0, 1]), ("R1", [2, 3])]);
        assert_eq!(map.elements[0].at, Point::new(32.5, 20.0));
    }

    #[test]
    fn auto_mode_without_pnp_uses_reading_order_and_warns() {
        let b = board(
            vec![hole(5.0, 5.0), hole(5.0, 30.0), hole(20.0, 30.0)],
            vec![],
        );
        let map = PinMap::build(&b, &settings(|_| {})).unwrap();
        assert_eq!(map.probes[0].hole.pos, Point::new(5.0, 30.0)); // top-left first
        assert_eq!(map.probes[2].hole.pos, Point::new(5.0, 5.0));
        assert!(map.elements.is_empty());
        assert!(map.warnings.iter().any(|w| w.contains("no elements")));
    }

    #[test]
    fn more_than_sixteen_holes_requires_selection() {
        let holes: Vec<Hole> = (0..17).map(|i| hole(f64::from(i) * 5.0, 10.0)).collect();
        let b = board(holes, vec![]);
        let e = PinMap::build(&b, &settings(|_| {}))
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("17 plated holes") && e.contains("--probe"),
            "{e}"
        );
        // ...but an explicit selection of up to 16 works
        let s = settings(|c| {
            c.probe = vec!["0=H1".parse().unwrap(), "15=H17".parse().unwrap()];
            c.element = vec![ElementSpec::parse_pair("VCC-GND=0,15", Typ::Linia).unwrap()];
        });
        let map = PinMap::build(&b, &s).unwrap();
        assert_eq!(map.probes.len(), 2);
        assert_eq!(map.elements[0].typ, Typ::Linia);
        assert_eq!(map.elements[0].at, Point::new(40.0, 10.0)); // midpoint of H1 and H17
    }

    #[test]
    fn exactly_sixteen_holes_are_accepted() {
        let holes: Vec<Hole> = (0..16).map(|i| hole(f64::from(i) * 5.0, 10.0)).collect();
        let map = PinMap::build(&board(holes, vec![]), &settings(|_| {})).unwrap();
        assert_eq!(map.probes.last().unwrap().pin, 15);
    }

    #[test]
    fn explicit_refs_by_pad_hole_and_coordinates() {
        let b = sample();
        let s = settings(|c| {
            c.probe = vec![
                "7=R1.2".parse().unwrap(),
                "8=H5".parse().unwrap(),
                "9=60.1,10.1".parse().unwrap(),
            ];
        });
        // 8=H5 and 9=(60.1,10.1) both name the same hole -> rejected
        assert!(
            PinMap::build(&b, &s)
                .unwrap_err()
                .to_string()
                .contains("would feed both")
        );
        let s = settings(|c| c.probe = vec!["7=R1.2".parse().unwrap(), "8=10,40".parse().unwrap()]);
        let map = PinMap::build(&b, &s).unwrap();
        assert_eq!(map.probes[0].hole.pos, Point::new(15.0, 40.0));
        assert_eq!(map.probes[1].hole.pos, Point::new(10.0, 40.0));
        assert_eq!(map.elements[0].name, "R1"); // derived from the component
        assert_eq!(map.elements[0].pins, [8, 7]);
    }

    #[test]
    fn selection_errors() {
        let b = sample();
        for (probe, needle) in [
            ("0=H99", "does not exist"),
            ("0=Q9.1", "no plated hole matches"),
            ("0=bogus", "cannot interpret"),
            ("0=1,1", "no plated hole within"),
        ] {
            let s = settings(|c| c.probe = vec![probe.parse().unwrap()]);
            let e = PinMap::build(&b, &s).unwrap_err().to_string();
            assert!(e.contains(needle), "{probe}: {e}");
        }
        let dup_pin =
            settings(|c| c.probe = vec!["0=H1".parse().unwrap(), "0=H2".parse().unwrap()]);
        assert!(
            PinMap::build(&b, &dup_pin)
                .unwrap_err()
                .to_string()
                .contains("assigned twice")
        );
    }

    #[test]
    fn element_validation_and_override() {
        let b = sample();
        let s = settings(|c| {
            c.element = vec![ElementSpec::parse_pair("VCC-GND=0,4", Typ::Linia).unwrap()];
        });
        let map = PinMap::build(&b, &s).unwrap();
        assert_eq!(map.elements.len(), 3);
        assert_eq!(map.elements[2].typ, Typ::Linia);
        // an explicit element with the same name replaces the derived one
        let s = settings(|c| {
            c.element = vec![ElementSpec::parse_pair("R1=2,4", Typ::Element).unwrap()];
        });
        let map = PinMap::build(&b, &s).unwrap();
        assert_eq!(map.elements.iter().filter(|e| e.name == "R1").count(), 1);
        assert_eq!(
            map.elements.iter().find(|e| e.name == "R1").unwrap().pins,
            [2, 4]
        );
        for (spec, needle) in [("X=1,1", "both pins"), ("X=1,12", "no probe assigned")] {
            let s = settings(|c| {
                c.element = vec![ElementSpec::parse_pair(spec, Typ::Element).unwrap()];
            });
            assert!(
                PinMap::build(&b, &s)
                    .unwrap_err()
                    .to_string()
                    .contains(needle),
                "{spec}"
            );
        }
        let no_auto = settings(|c| c.auto_elements = Some(false));
        assert!(PinMap::build(&b, &no_auto).unwrap().elements.is_empty());
    }

    #[test]
    fn multi_pad_component_warns() {
        let b = board(
            vec![hole(10.0, 10.0), hole(12.0, 10.0), hole(14.0, 10.0)],
            vec![comp("U1", 12.0, 10.0)],
        );
        let map = PinMap::build(&b, &settings(|_| {})).unwrap();
        assert!(map.elements.is_empty());
        assert!(map.warnings.iter().any(|w| w.contains("U1: 3 probed pads")));
    }

    #[test]
    fn json_has_exact_shape_and_utf8() {
        let lista = ListaElementow {
            elementy: vec![
                Wpis {
                    nazwa: "R1".into(),
                    piny: [0, 1],
                    x: 168,
                    y: 240,
                    typ: Typ::Element,
                },
                Wpis {
                    nazwa: "Rezystor łączący".into(),
                    piny: [0, 15],
                    x: 256,
                    y: 452,
                    typ: Typ::Linia,
                },
            ],
        };
        let json = lista.to_json().unwrap();
        assert!(json.contains("Rezystor łączący"), "{json}");
        assert!(!json.contains("\\u"));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.len(), 1);
        let first = v["elementy"][0].as_object().unwrap();
        let mut keys: Vec<_> = first.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["nazwa", "piny", "typ", "x", "y"]); // exactly these five keys
        assert_eq!(v["elementy"][1]["typ"], "linia");
        assert!(v["elementy"][0]["x"].is_u64());
        assert!(json.contains(r#"{"nazwa":"R1","piny":[0,1],"x":168,"y":240,"typ":"element"}"#));
        assert_eq!(
            ListaElementow { elementy: vec![] }.to_json().unwrap(),
            "{\n  \"elementy\": []\n}\n"
        );
    }

    #[test]
    fn validate_rejects_contract_violations() {
        let bad = |piny: [u8; 2], nazwa: &str| ListaElementow {
            elementy: vec![Wpis {
                nazwa: nazwa.into(),
                piny,
                x: 0,
                y: 0,
                typ: Typ::Element,
            }],
        };
        assert!(bad([3, 3], "a").validate().is_err());
        assert!(bad([0, 16], "a").validate().is_err());
        assert!(bad([0, 1], " ").validate().is_err());
        assert!(bad([15, 0], "a").validate().is_ok());
    }
}
