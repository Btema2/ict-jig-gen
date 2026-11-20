//! The parsed board: outline extents, drilled holes and (optionally) component centroids.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::excellon::{self, Hole};
use crate::geom::{BBox, Point};
use crate::gerber;
use crate::package::{Layer, Package};
use crate::pnp::{self, Component, PickAndPlace};

/// Plated holes at least this large are mounting holes when no NPTH file exists.
const MOUNTING_HOLE_MIN_DIAMETER: f64 = 2.5;
/// Holes may sit this far outside the outline box (rounding / edge-plated pads).
const OUTSIDE_TOLERANCE_MM: f64 = 1.0;
/// Two holes closer than this (and equally sized) are the same hole listed twice.
const DUPLICATE_TOLERANCE_MM: f64 = 0.01;

/// Everything known about the board to be tested. All coordinates are board millimetres
/// (Gerber frame: +Y up).
#[derive(Debug, Clone)]
pub struct Board {
    /// Package the board came from.
    pub source: String,
    /// Outline bounding box.
    pub bbox: BBox,
    /// Plated through-holes (probe targets), in reading order: top row first, left to right.
    pub plated: Vec<Hole>,
    /// Mounting holes usable for alignment posts, in reading order.
    pub mounting: Vec<Hole>,
    /// Component centroids from the Pick & Place file, if provided.
    pub components: Vec<Component>,
    /// Non-fatal observations to show the user.
    pub warnings: Vec<String>,
}

impl Board {
    /// Parse the layers of a package.
    pub fn from_package(pkg: &Package) -> Result<Self> {
        let outline = gerber::parse_outline(&text(&pkg.outline), &pkg.outline.name)?;
        let mut warnings = Vec::new();
        let mut plated = load_drills(&pkg.plated, &outline.bbox, &mut warnings)?;
        let mut mounting = load_drills(&pkg.non_plated, &outline.bbox, &mut warnings)?;

        if mounting.is_empty() {
            let (large, small): (Vec<Hole>, Vec<Hole>) = plated
                .iter()
                .partition(|h| h.diameter >= MOUNTING_HOLE_MIN_DIAMETER);
            if !large.is_empty() {
                warnings.push(format!(
                    "no non-plated drill file: using {} plated hole(s) >= {MOUNTING_HOLE_MIN_DIAMETER} mm \
                     as mounting holes",
                    large.len()
                ));
                mounting = large;
                plated = small;
            }
        }

        if plated.is_empty() {
            let name = pkg
                .plated
                .first()
                .map_or("plated drill file", |l| l.name.as_str());
            return Err(Error::parse(name, "no plated holes found"));
        }

        let mut board = Self {
            source: pkg.source.clone(),
            bbox: outline.bbox,
            plated,
            mounting,
            components: Vec::new(),
            warnings,
        };
        if let Some(layer) = &pkg.pick_and_place {
            board.set_components(pnp::parse(&layer.bytes, &layer.name)?, &layer.name);
        }
        Ok(board)
    }

    /// Attach component centroids (overrides any earlier set).
    pub fn set_components(&mut self, parsed: PickAndPlace, file: &str) {
        let PickAndPlace {
            components,
            warnings,
        } = parsed;
        self.warnings
            .extend(warnings.into_iter().map(|w| format!("{file}: {w}")));
        let inside = components
            .iter()
            .filter(|c| self.bbox.contains(c.pos, 5.0))
            .count();
        if inside * 2 < components.len() {
            self.warnings.push(format!(
                "{file}: only {inside} of {} component centroids fall on the board; the Pick & Place \
                 origin probably differs from the Gerber origin, so designators may be mismatched",
                components.len()
            ));
        }
        self.components = components;
    }
}

fn text(layer: &Layer) -> String {
    String::from_utf8_lossy(&layer.bytes).into_owned()
}

fn load_drills(layers: &[Layer], bbox: &BBox, warnings: &mut Vec<String>) -> Result<Vec<Hole>> {
    // grid of DUPLICATE_TOLERANCE_MM-sized cells so de-duplication stays near-linear
    let cell = |v: f64| (v / DUPLICATE_TOLERANCE_MM).floor();
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut holes: Vec<Hole> = Vec::new();
    for layer in layers {
        let parsed = excellon::parse(&text(layer), &layer.name)?;
        if parsed.skipped_slots > 0 {
            warnings.push(format!(
                "{}: ignored {} slot(s)",
                layer.name, parsed.skipped_slots
            ));
        }
        for hole in parsed.holes {
            if !bbox.contains(hole.pos, OUTSIDE_TOLERANCE_MM) {
                return Err(Error::parse(
                    &layer.name,
                    format!(
                        "hole at ({:.3}, {:.3}) mm lies outside the board outline \
                         ({:.3}..{:.3}, {:.3}..{:.3}); drill and outline files use different origins",
                        hole.pos.x, hole.pos.y, bbox.min.x, bbox.max.x, bbox.min.y, bbox.max.y
                    ),
                ));
            }
            // cell indices are small (|coordinate| <= 5000 mm / 0.01 mm), so the cast is exact
            #[allow(clippy::cast_possible_truncation)]
            let key = (cell(hole.pos.x) as i64, cell(hole.pos.y) as i64);
            let duplicate = (key.0 - 1..=key.0 + 1)
                .flat_map(|cx| (key.1 - 1..=key.1 + 1).map(move |cy| (cx, cy)))
                .filter_map(|k| grid.get(&k))
                .flatten()
                .any(|&i| {
                    let h = &holes[i];
                    h.pos.distance(hole.pos) < DUPLICATE_TOLERANCE_MM
                        && (h.diameter - hole.diameter).abs() < DUPLICATE_TOLERANCE_MM
                });
            if !duplicate {
                grid.entry(key).or_default().push(holes.len());
                holes.push(hole);
            }
        }
    }
    sort_reading_order(&mut holes);
    Ok(holes)
}

/// Top row first (largest Y), left to right; rows are matched to 0.05 mm.
pub fn sort_reading_order(holes: &mut [Hole]) {
    holes.sort_by(|a, b| {
        let row = |p: Point| (-p.y / 0.05).round();
        row(a.pos)
            .total_cmp(&row(b.pos))
            .then(a.pos.x.total_cmp(&b.pos.x))
    });
}
