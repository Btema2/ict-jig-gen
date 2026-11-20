//! The parsed board: outline extents, drilled holes and (optionally) component centroids.

use crate::error::{Error, Result};
use crate::excellon::{self, Hole};
use crate::geom::{BBox, Point};
use crate::gerber;
use crate::package::{Layer, Package};
use crate::pnp::{self, Component};

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
        let plated = load_drills(&pkg.plated, &mut warnings)?;
        let mut mounting = load_drills(&pkg.non_plated, &mut warnings)?;
        let mut plated = plated;

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

        for hole in plated.iter().chain(&mounting) {
            if !outline.bbox.contains(hole.pos, OUTSIDE_TOLERANCE_MM) {
                return Err(Error::parse(
                    &pkg.plated[0].name,
                    format!(
                        "hole at ({:.3}, {:.3}) mm lies outside the board outline \
                         ({:.3}..{:.3}, {:.3}..{:.3}); drill and outline files use different origins",
                        hole.pos.x,
                        hole.pos.y,
                        outline.bbox.min.x,
                        outline.bbox.max.x,
                        outline.bbox.min.y,
                        outline.bbox.max.y
                    ),
                ));
            }
        }
        if plated.is_empty() {
            return Err(Error::parse(&pkg.plated[0].name, "no plated holes found"));
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
    pub fn set_components(&mut self, components: Vec<Component>, file: &str) {
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

fn load_drills(layers: &[Layer], warnings: &mut Vec<String>) -> Result<Vec<Hole>> {
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
            let duplicate = holes.iter().any(|h| {
                h.pos.distance(hole.pos) < DUPLICATE_TOLERANCE_MM
                    && (h.diameter - hole.diameter).abs() < DUPLICATE_TOLERANCE_MM
            });
            if !duplicate {
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
