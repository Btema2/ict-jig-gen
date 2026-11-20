//! The two-layer pogo-pin cassette.
//!
//! ```text
//!   Z up (print orientation)                    section through a pogo hole
//!
//!        post  lip                                 ┌─ plunger tip (above the board seat)
//!         ┃   ┌──┐  ┌─────────────┐  ┌──┐          ─┴─
//!   z3 ━━━┻━━━┘  └──┘ top guide   └──┘  └━━━        │ │  top guide plate (z2..z3)
//!   z2 ───────────────────────────────────         ─┘ └─
//!        pillar          open cavity    pillar       │ │  <- barrel unsupported, wires exit sideways
//!   z1 ───────────────────────────────────         ─┐ ┌─
//!        bottom guide plate                           │ │  bottom guide plate (z0..z1)
//!   z0 ═══════════════════════════════════         ─┘ └─  flat base on Z = 0
//! ```
//!
//! Both plates carry identical hole patterns, so every pogo barrel is held in two concentric
//! bores `standoff_height` apart and cannot tilt. The plates are joined by square pillars in
//! the margin around the board, leaving the cavity open on every side for wiring and for
//! removing support material. The board rests on the top plate and is located either by
//! posts that enter its mounting holes or by a low lip around its outline.
//!
//! With `split` the same cassette is emitted as two parts that each print flat on `Z = 0`
//! without supports (see [`Joint`]): the bottom plate with its pillars, and the top plate
//! shifted down to `Z = 0` (underside on the bed, posts / lip up).
//!
//! With `compact` the footprint shrinks to the board outline and the perimeter pillars give
//! way to a [`SteppedPillar`] on every mounting hole: a round shoulder between the plates and a
//! narrower pin that carries on through the top plate as the alignment post. Combined with
//! `split`, the pins belong to the bottom part and the flat top plate slides down over them
//! until it rests on the shoulders, so no screws are needed:
//!
//! ```text
//!            ╻ post (tapered tip)          section through a compact pillar, split
//!   z3 ━━━━━━┃━━━━━━━━━━━━  top plate: pogo bores + a socket around each pin
//!   z2 ━━━━━━┃━━━━━━━━━━━━
//!          ┌─┸─┐ shoulder (the top plate rests on its ledge)
//!   z1 ━━━━┷━━━┷━━━━━━━━━━  bottom plate
//!   z0 ━━━━━━━━━━━━━━━━━━━
//! ```
//!
//! Coordinates: the cassette origin is the lower-left corner of the footprint; board
//! coordinates map in with a pure translation (no mirroring: the board lies component-side up
//! on the cassette, probes touch its underside pads).

use crate::board::Board;
use crate::config::{Alignment, Mechanical};
use crate::error::{Error, Result};
use crate::geom::Point;
use crate::mesh::{Mesh, P2, Region, circle, rect};
use crate::pinmap::Probe;

/// Edge length of a support pillar.
const PILLAR_SIZE: f64 = 5.0;
/// Gap between a pillar and the plate edge.
const PILLAR_INSET: f64 = 1.5;
/// Longest unsupported stretch along a plate edge before an extra pillar is added.
const MAX_PILLAR_SPAN: f64 = 40.0;
/// Free space between the board edge and the lip's inner face.
const POCKET_CLEARANCE: f64 = 0.3;
/// Thickness of the registration lip.
const LIP_WALL: f64 = 2.0;
/// Smallest mounting hole that gets an alignment post.
const MIN_POST_HOLE_DIAMETER: f64 = 1.5;
/// How far a post sticks out above the board surface.
const POST_PROTRUSION: f64 = 1.0;
/// Height of the post's tapered tip.
const POST_TIP_HEIGHT: f64 = 0.8;
/// Extra distance between a feature and the lip wall.
const LIP_FEATURE_GAP: f64 = 0.4;
/// Width of the ledge from a compact pillar's pin socket out to its shoulder: where the top
/// plate rests.
pub const SHOULDER_LEDGE: f64 = 1.0;
/// Fewest mounting holes `compact` can stand the plates on.
pub const MIN_COMPACT_PILLARS: usize = 2;

/// File name of the combined cassette (default mode).
pub const STL_NAME: &str = "cassette.stl";
/// File name of the bottom half with `split`.
pub const STL_BOTTOM_NAME: &str = "cassette_bottom.stl";
/// File name of the top half with `split`.
pub const STL_TOP_NAME: &str = "cassette_top.stl";

/// Pilot hole in each fastening boss, sized for a thread-forming M3 screw in PLA.
pub const SCREW_PILOT_DIAMETER: f64 = 2.6;
/// Thinnest wall around the pilot hole, whatever `--min-wall` says (a thread-forming screw
/// splits anything thinner).
const MIN_BOSS_WALL: f64 = 0.7;
/// How much bigger than a plain socket the keyed boss is, so it cannot enter one.
const KEY_STEP: f64 = 0.3;
/// Air kept between a screw head and the lip's outer corner.
const HEAD_LIP_GAP: f64 = 0.1;
/// Gap kept between a boss and the edge of its pillar.
const BOSS_EDGE_GAP: f64 = 0.1;
/// Longest locating boss.
const BOSS_MAX_HEIGHT: f64 = 2.0;
/// The boss stops at least this far below the top-plate surface, so the screw head clamps the
/// plate instead of the boss.
const BOSS_RECESS: f64 = 0.5;
/// Largest M3 head allowed for (pan, button and cap heads are 5.5..5.7 mm).
pub const SCREW_HEAD_DIAMETER: f64 = 5.7;
/// The screw tip stays this far above the floor of the pilot hole.
const PILOT_FLOOR_GAP: f64 = 1.0;
/// Fewer mm of thread in the bosses than this earns a warning.
const MIN_SCREW_ENGAGEMENT: f64 = 4.0;
/// Standard M3 screw lengths, mm, ascending.
const SCREW_LENGTHS: [u32; 8] = [6, 8, 10, 12, 16, 20, 25, 30];

/// An alignment post.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Post {
    /// Centre in cassette coordinates.
    pub center: Point,
    /// Shaft diameter.
    pub diameter: f64,
}

/// A `compact` support pillar on one of the board's mounting holes.
///
/// Between the plates it is a round shoulder; above the shoulder a pin of the alignment post's
/// diameter rises through the top plate and ends in the post's tapered tip. One-piece, the pin
/// is fused into the top plate; with `split` it belongs to the bottom half and passes through a
/// socket in the top plate, which rests on the shoulder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SteppedPillar {
    /// Centre in cassette coordinates (the mounting hole's centre).
    pub center: Point,
    /// Pin and alignment-post diameter: the mounting hole minus `clearance`.
    pub pin_diameter: f64,
    /// Socket through a split top plate: the pin plus `clearance` (a slip fit).
    pub socket_diameter: f64,
    /// Shoulder between the plates: the socket plus a [`SHOULDER_LEDGE`] on every side.
    pub shoulder_diameter: f64,
}

impl SteppedPillar {
    /// The pillar under `post` (any coordinate frame; the centre is copied).
    fn under(post: &Post, mech: &Mechanical) -> Self {
        let socket_diameter = post.diameter + mech.clearance;
        Self {
            center: post.center,
            pin_diameter: post.diameter,
            socket_diameter,
            shoulder_diameter: socket_diameter + 2.0 * SHOULDER_LEDGE,
        }
    }
}

/// Axis-aligned rectangle in cassette coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Lower-left corner.
    pub min: Point,
    /// Upper-right corner.
    pub max: Point,
}

impl Rect {
    fn distance_to(&self, p: Point) -> f64 {
        let dx = (self.min.x - p.x).max(0.0).max(p.x - self.max.x);
        let dy = (self.min.y - p.y).max(0.0).max(p.y - self.max.y);
        dx.hypot(dy)
    }

    fn ring(&self) -> Vec<P2> {
        rect([self.min.x, self.min.y], [self.max.x, self.max.y])
    }

    fn contains(&self, p: Point) -> bool {
        self.distance_to(p) == 0.0
    }
}

/// One M3 fastening point of a split cassette: a locating boss on a bottom-half corner pillar
/// with a pilot hole for the screw, and a matching through-socket in the top half.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fastener {
    /// Centre in cassette coordinates (the top and bottom halves share the XY frame).
    pub center: Point,
    /// The lower-left fastener has the larger boss, so the top half cannot be fitted rotated.
    pub keyed: bool,
    /// Boss diameter on the bottom half.
    pub boss_diameter: f64,
}

/// How the two halves of a `--split` cassette are joined by M3 screws (without `compact`;
/// compact halves are held by the [`SteppedPillar`] pins instead).
///
/// Both halves print flat on `Z = 0` (every downward-facing surface is on the bed). In the
/// assembled cassette the top half sits `z_top_plate_bottom` higher than it prints.
#[derive(Debug, Clone, PartialEq)]
pub struct Joint {
    /// Bosses / screws, one per corner pillar.
    pub fasteners: Vec<Fastener>,
    /// Boss height above the pillar top. It fills the socket for this much of the plate
    /// thickness and stops short of the plate surface.
    pub boss_height: f64,
    /// Pilot hole depth, measured down from the boss top to the pillar base.
    pub pilot_depth: f64,
    /// Socket diameter = boss diameter + this (the same slip-fit allowance as the pogo bores).
    pub socket_clearance: f64,
    /// Longest standard M3 screw that fits the pilot hole, if any does.
    pub screw_length: Option<u32>,
}

impl Joint {
    /// Diameter of the socket cut through the top plate for `f`.
    pub fn socket_diameter(&self, f: &Fastener) -> f64 {
        f.boss_diameter + self.socket_clearance
    }
}

/// Dimensions and feature positions of the generated cassette (cassette coordinates, mm).
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Footprint size along X.
    pub width: f64,
    /// Footprint size along Y.
    pub depth: f64,
    /// Overall height including lip / posts.
    pub height: f64,
    /// Plate beyond the board outline's bounding box on every side.
    pub margin: f64,
    /// Translation from board to cassette coordinates.
    pub offset: Point,
    /// Top of the bottom plate.
    pub z_bottom_plate_top: f64,
    /// Underside of the top plate.
    pub z_top_plate_bottom: f64,
    /// Top plate surface (the board seat).
    pub z_top_plate_top: f64,
    /// Diameter of each pogo hole.
    pub hole_diameter: f64,
    /// Pogo hole centres, in the same order as the probes passed to [`build`].
    pub holes: Vec<Point>,
    /// Alignment posts.
    pub posts: Vec<Post>,
    /// Registration pocket (inner face of the lip), if a lip was generated.
    pub pocket: Option<Rect>,
    /// Square support pillars in the margin (empty with `compact`).
    pub pillars: Vec<Rect>,
    /// `compact` only: a stepped pillar under every post, in the same order as `posts`.
    pub stepped_pillars: Vec<SteppedPillar>,
    /// Screw joint between the halves (`split` without `compact`; a compact split cassette
    /// is held together by the stepped pillars' pins instead).
    pub joint: Option<Joint>,
}

/// The printable solid(s).
#[derive(Debug, Clone)]
pub enum Solids {
    /// One combined cassette in assembled orientation.
    Single(Mesh),
    /// Two halves, each oriented to print flat on `Z = 0`.
    Split {
        /// Bottom guide plate + pillars + locating bosses.
        bottom: Mesh,
        /// Top guide plate + posts / lip, printed underside-down.
        top: Mesh,
    },
}

impl Solids {
    /// The meshes with their output file names (`[cassette]` or `[bottom, top]`).
    pub fn files(&self) -> Vec<(&'static str, &Mesh)> {
        match self {
            Self::Single(m) => vec![(STL_NAME, m)],
            Self::Split { bottom, top } => vec![(STL_BOTTOM_NAME, bottom), (STL_TOP_NAME, top)],
        }
    }

    /// Plastic in all parts together, mm³ (solid, i.e. before infill).
    pub fn volume(&self) -> f64 {
        self.files().iter().map(|(_, m)| m.signed_volume()).sum()
    }
}

/// A `compact` cassette compared with the default one the same settings would give.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Savings {
    /// Footprint of the default cassette along X, mm.
    pub default_width: f64,
    /// Footprint of the default cassette along Y, mm.
    pub default_depth: f64,
    /// Plastic in the default cassette (all parts), mm³.
    pub default_volume: f64,
    /// Plastic in this cassette (all parts), mm³.
    pub volume: f64,
}

/// Generated cassette.
#[derive(Debug, Clone)]
pub struct Cassette {
    /// The solid(s).
    pub solids: Solids,
    /// Where everything is.
    pub layout: Layout,
    /// Non-fatal observations.
    pub warnings: Vec<String>,
    /// With `compact`: how it compares with the default cassette (`None` otherwise, or when
    /// the default cassette cannot be built for this board).
    pub savings: Option<Savings>,
}

/// Build the cassette for the given probes (one hole per probe, in order).
pub fn build(board: &Board, probes: &[Probe], mech: &Mechanical) -> Result<Cassette> {
    let mut cassette = build_one(board, probes, mech)?;
    if mech.compact {
        let default = Mechanical {
            compact: false,
            margin: Mechanical::default().margin,
            ..mech.clone()
        };
        cassette.savings = build_one(board, probes, &default).ok().map(|d| Savings {
            default_width: d.layout.width,
            default_depth: d.layout.depth,
            default_volume: d.solids.volume(),
            volume: cassette.solids.volume(),
        });
    }
    Ok(cassette)
}

fn build_one(board: &Board, probes: &[Probe], mech: &Mechanical) -> Result<Cassette> {
    if probes.is_empty() {
        return Err(Error::Geometry("no probe points selected".into()));
    }
    let mut warnings = Vec::new();
    let (mut posts, use_lip) = plan_alignment(board, mech, &mut warnings)?;
    let mut stepped_pillars: Vec<SteppedPillar> = if mech.compact {
        posts
            .iter()
            .map(|p| SteppedPillar::under(p, mech))
            .collect()
    } else {
        Vec::new()
    };
    let margin = if mech.compact {
        compact_margin(board, probes, &stepped_pillars, mech, &mut warnings)
    } else {
        effective_margin(mech, use_lip, &mut warnings)
    };
    let offset = Point::new(margin - board.bbox.min.x, margin - board.bbox.min.y);
    let to_cassette = |p: Point| Point::new(p.x + offset.x, p.y + offset.y);
    for post in &mut posts {
        post.center = to_cassette(post.center);
    }
    for pillar in &mut stepped_pillars {
        pillar.center = to_cassette(pillar.center);
    }
    let (width, depth) = (
        board.bbox.width() + 2.0 * margin,
        board.bbox.height() + 2.0 * margin,
    );

    let holes: Vec<Point> = probes.iter().map(|p| to_cassette(p.hole.pos)).collect();
    let pocket = use_lip.then(|| {
        let min = Point::new(margin - POCKET_CLEARANCE, margin - POCKET_CLEARANCE);
        let max = Point::new(
            width - margin + POCKET_CLEARANCE,
            depth - margin + POCKET_CLEARANCE,
        );
        Rect { min, max }
    });
    let pillars = if mech.compact {
        Vec::new()
    } else {
        plan_pillars(width, depth)
    };
    let joint = if mech.split && !mech.compact {
        Some(plan_joint(width, depth, mech, &mut warnings)?)
    } else {
        None
    };
    let z1 = mech.plate_thickness;
    let z2 = z1 + mech.standoff_height;
    let z3 = z2 + mech.plate_thickness;
    let post_height = mech.pcb_thickness + POST_PROTRUSION;
    let top = z3
        + if use_lip { mech.lip_height } else { 0.0 }.max(if posts.is_empty() {
            0.0
        } else {
            post_height
        });
    let layout = Layout {
        width,
        depth,
        height: top,
        margin,
        offset,
        z_bottom_plate_top: z1,
        z_top_plate_bottom: z2,
        z_top_plate_top: z3,
        hole_diameter: mech.hole_diameter(),
        holes,
        posts,
        pocket,
        pillars,
        stepped_pillars,
        joint,
    };
    check_clearances(&layout, probes, mech)?;
    stack_up_warning(mech, &mut warnings);
    if mech.split {
        one_line_warning(&layout.stepped_pillars, &mut warnings);
    }

    let solids = assemble(&layout, mech)?;
    for (_, mesh) in solids.files() {
        let topology = mesh.topology();
        if !topology.is_watertight() {
            return Err(Error::Geometry(format!(
                "internal error: generated mesh is not watertight ({topology:?}); refusing to write it"
            )));
        }
    }
    Ok(Cassette {
        solids,
        layout,
        warnings,
        savings: None,
    })
}

/// Distance from `p` to the nearest side of the board outline's bounding box (negative
/// outside it).
fn inset(board: &Board, p: Point) -> f64 {
    let b = &board.bbox;
    (p.x - b.min.x)
        .min(b.max.x - p.x)
        .min(p.y - b.min.y)
        .min(b.max.y - p.y)
}

/// The margin for `compact`: the requested one (0 unless `--margin` is given), widened, with a
/// warning, if a pogo bore or a pillar shoulder would otherwise come closer than `--min-wall` to
/// the cassette edge. `pillars` are in board coordinates.
fn compact_margin(
    board: &Board,
    probes: &[Probe],
    pillars: &[SteppedPillar],
    mech: &Mechanical,
    warnings: &mut Vec<String>,
) -> f64 {
    let hole_r = mech.hole_diameter() / 2.0;
    let bores = probes.iter().map(|p| {
        (
            hole_r - inset(board, p.hole.pos),
            format!("pin {} ({})", p.pin, p.label),
        )
    });
    let shoulders = pillars.iter().map(|s| {
        (
            s.shoulder_diameter / 2.0 - inset(board, s.center),
            format!(
                "the pillar on the mounting hole at ({:.2}, {:.2})",
                s.center.x, s.center.y
            ),
        )
    });
    let Some((reach, what)) = bores.chain(shoulders).max_by(|a, b| a.0.total_cmp(&b.0)) else {
        return mech.margin;
    };
    let needed = reach + mech.min_wall;
    if needed <= mech.margin + 1e-9 {
        return mech.margin;
    }
    // round up to 0.1 mm (the epsilon keeps an exact 0.9 from becoming 1.0)
    let widened = ((needed - 1e-9) * 10.0).ceil() / 10.0;
    warnings.push(format!(
        "--compact: {what} needs {:.2} mm of plate beyond the board outline (--min-wall {} from \
         the cassette edge); using a {widened:.1} mm margin instead of {:.1} mm",
        needed.max(0.0),
        mech.min_wall,
        mech.margin
    ));
    widened
}

/// A split compact top plate rests on the shoulders only. If they all stand in (nearly) one
/// line, nothing props it up on either side of that line.
fn one_line_warning(pillars: &[SteppedPillar], warnings: &mut Vec<String>) {
    let mut far: Option<(f64, Point, Point)> = None;
    for (i, a) in pillars.iter().enumerate() {
        for b in pillars.iter().skip(i + 1) {
            let d = a.center.distance(b.center);
            if far.is_none_or(|(best, _, _)| d > best) {
                far = Some((d, a.center, b.center));
            }
        }
    }
    let Some((length, a, b)) = far.filter(|f| f.0 > 0.0) else {
        return;
    };
    let off_line =
        |p: Point| ((b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)).abs() / length;
    let spread = pillars
        .iter()
        .map(|s| off_line(s.center))
        .fold(0.0, f64::max);
    let width = pillars
        .iter()
        .map(|s| s.shoulder_diameter)
        .fold(0.0, f64::max);
    if spread < width {
        warnings.push(format!(
            "--compact --split: the {} pillars stand in one line, so the loose top plate rests on \
             that line only and can tip under an off-centre press; press the board down close to \
             the posts",
            pillars.len()
        ));
    }
}

/// The margin to build with. In split mode with a registration lip, the M3 screw head on each
/// corner pillar must clear the lip's outer corner (the nearest part of the lip). Without a lip
/// the screw heads clear the board seat for every margin `Mechanical::validate` accepts.
fn effective_margin(mech: &Mechanical, use_lip: bool, warnings: &mut Vec<String>) -> f64 {
    if !(mech.split && use_lip) {
        return mech.margin;
    }
    let centre = PILLAR_INSET + PILLAR_SIZE / 2.0;
    let lip_corner = POCKET_CLEARANCE + LIP_WALL;
    let needed =
        centre + lip_corner + SCREW_HEAD_DIAMETER / 2.0 / std::f64::consts::SQRT_2 + HEAD_LIP_GAP;
    if mech.margin < needed {
        warnings.push(format!(
            "--split with a registration lip needs a {needed:.2} mm margin for the M3 screw heads; \
             using it instead of {:.2} mm",
            mech.margin
        ));
        return needed;
    }
    mech.margin
}

fn plan_joint(
    width: f64,
    depth: f64,
    mech: &Mechanical,
    warnings: &mut Vec<String>,
) -> Result<Joint> {
    // The keyed boss (plain + clearance + KEY_STEP) must still fit the pillar, which caps the
    // plain boss; its wall is as thick as `--min-wall` asks for, if the pillar allows.
    let largest = PILLAR_SIZE - 2.0 * BOSS_EDGE_GAP;
    let widest_plain = largest - mech.clearance - KEY_STEP;
    let wanted = SCREW_PILOT_DIAMETER + 2.0 * mech.min_wall.max(MIN_BOSS_WALL);
    let plain = wanted.min(widest_plain);
    let wall = (plain - SCREW_PILOT_DIAMETER) / 2.0;
    if wall < MIN_BOSS_WALL - 1e-9 {
        return Err(Error::Geometry(format!(
            "--split: with --clearance {} the keyed locating boss leaves only {wall:.2} mm of wall \
             around the screw hole in a {PILLAR_SIZE} mm pillar (needs {MIN_BOSS_WALL} mm); reduce \
             --clearance",
            mech.clearance
        )));
    }
    if wall < mech.min_wall - 1e-9 {
        warnings.push(format!(
            "--split: the locating bosses have {wall:.2} mm walls, thinner than --min-wall {} \
             (the {PILLAR_SIZE} mm pillars leave no more room)",
            mech.min_wall
        ));
    }
    let keyed = plain + mech.clearance + KEY_STEP;
    let c = PILLAR_INSET + PILLAR_SIZE / 2.0;
    let fasteners = [
        (c, c),
        (width - c, c),
        (c, depth - c),
        (width - c, depth - c),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (x, y))| Fastener {
        center: Point::new(x, y),
        keyed: i == 0,
        boss_diameter: if i == 0 { keyed } else { plain },
    })
    .collect();
    let plate = mech.plate_thickness;
    let boss_height = (plate - BOSS_RECESS).min(BOSS_MAX_HEIGHT);
    let longest = plate + mech.standoff_height - PILOT_FLOOR_GAP;
    let screw_length = SCREW_LENGTHS
        .into_iter()
        .rev()
        .find(|&l| f64::from(l) <= longest);
    let engagement = screw_length.map_or(0.0, |l| f64::from(l) - (plate - boss_height));
    if engagement < MIN_SCREW_ENGAGEMENT {
        warnings.push(format!(
            "--split: the pillars are too short for a reliable M3 joint ({engagement:.1} mm of \
             thread in the bosses, wanted {MIN_SCREW_ENGAGEMENT} mm); increase --standoff-height"
        ));
    }
    Ok(Joint {
        fasteners,
        boss_height,
        pilot_depth: boss_height + mech.standoff_height,
        socket_clearance: mech.clearance,
        screw_length,
    })
}

/// Posts are returned in board coordinates.
fn plan_alignment(
    board: &Board,
    mech: &Mechanical,
    warnings: &mut Vec<String>,
) -> Result<(Vec<Post>, bool)> {
    let eligible: Vec<Post> = board
        .mounting
        .iter()
        .filter(|h| h.diameter >= MIN_POST_HOLE_DIAMETER)
        .map(|h| Post {
            center: h.pos, // board coordinates; the caller translates
            diameter: h.diameter - mech.clearance,
        })
        .collect();
    if mech.compact {
        // `Mechanical::validate` already limits the alignment to auto / posts
        if eligible.len() < MIN_COMPACT_PILLARS {
            return Err(Error::Geometry(format!(
                "--compact stands the plates on pillars at the board's mounting holes and needs at \
                 least {MIN_COMPACT_PILLARS} non-plated holes >= {MIN_POST_HOLE_DIAMETER} mm, but \
                 the board has {}; drop --compact to build the default cassette, which locates the \
                 board with a registration lip instead",
                eligible.len()
            )));
        }
        return Ok((eligible, false));
    }
    let (use_posts, use_lip) = match mech.alignment {
        Alignment::None => (false, false),
        Alignment::Lip => (false, true),
        Alignment::Posts => {
            if eligible.is_empty() {
                return Err(Error::Geometry(format!(
                    "alignment = posts needs a non-plated hole >= {MIN_POST_HOLE_DIAMETER} mm, but the board has none; use --alignment lip"
                )));
            }
            (true, false)
        }
        Alignment::Both => (!eligible.is_empty(), true),
        Alignment::Auto => (!eligible.is_empty(), eligible.len() < 2),
    };
    if use_posts && eligible.len() == 1 && !use_lip {
        warnings.push(
            "a single alignment post cannot stop the board rotating; consider --alignment both"
                .into(),
        );
    }
    if mech.alignment == Alignment::Both && eligible.is_empty() {
        warnings.push("no mounting holes found: generating the registration lip only".into());
    }
    Ok((if use_posts { eligible } else { Vec::new() }, use_lip))
}

fn plan_pillars(width: f64, depth: f64) -> Vec<Rect> {
    let half = PILLAR_SIZE / 2.0;
    let c = PILLAR_INSET + half;
    let spread = |lo: f64, hi: f64| -> Vec<f64> {
        let segments = ((hi - lo) / MAX_PILLAR_SPAN).ceil().max(1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // small positive count
        let n = segments as usize;
        #[allow(clippy::cast_precision_loss)]
        (0..=n)
            .map(|i| lo + (hi - lo) * i as f64 / n as f64)
            .collect()
    };
    let square = |cx: f64, cy: f64| Rect {
        min: Point::new(cx - half, cy - half),
        max: Point::new(cx + half, cy + half),
    };
    let xs = spread(c, width - c);
    let ys = spread(c, depth - c);
    let mut out = Vec::new();
    for &x in &xs {
        out.push(square(x, c));
        out.push(square(x, depth - c));
    }
    for &y in ys.iter().skip(1).take(ys.len().saturating_sub(2)) {
        out.push(square(c, y));
        out.push(square(width - c, y));
    }
    out
}

fn check_clearances(layout: &Layout, probes: &[Probe], mech: &Mechanical) -> Result<()> {
    // first: a compact shoulder is wider than its post, so the message names the real culprit
    check_stepped_pillars(layout, probes, mech)?;
    let hole_r = layout.hole_diameter / 2.0;
    let min_pitch = layout.hole_diameter + mech.min_wall;
    for (i, (a, pa)) in layout.holes.iter().zip(probes).enumerate() {
        for (b, pb) in layout.holes.iter().zip(probes).skip(i + 1) {
            let d = a.distance(*b);
            if d < min_pitch {
                return Err(Error::Geometry(format!(
                    "pins {} ({}) and {} ({}) are {d:.2} mm apart, closer than the {min_pitch:.2} mm a \
                     {:.2} mm hole plus a {} mm wall needs; choose other test points, a thinner pogo \
                     or a smaller --clearance/--min-wall",
                    pa.pin, pa.label, pb.pin, pb.label, layout.hole_diameter, mech.min_wall
                )));
            }
        }
        for post in &layout.posts {
            let need = hole_r + post.diameter / 2.0 + mech.min_wall;
            if a.distance(post.center) < need {
                return Err(Error::Geometry(format!(
                    "pin {} ({}) is too close to an alignment post at ({:.2}, {:.2}) mm (needs {need:.2} mm)",
                    pa.pin, pa.label, post.center.x, post.center.y
                )));
            }
        }
        if let Some(p) = layout
            .pillars
            .iter()
            .find(|p| p.distance_to(*a) < hole_r + mech.min_wall)
        {
            return Err(Error::Geometry(format!(
                "pin {} ({}) collides with a support pillar at ({:.1}, {:.1}) mm; increase --margin",
                pa.pin, pa.label, p.min.x, p.min.y
            )));
        }
        if let Some(pocket) = &layout.pocket {
            inside_pocket(
                pocket,
                *a,
                hole_r,
                &format!("pin {} ({})", pa.pin, pa.label),
            )?;
        }
    }
    if let Some(pocket) = &layout.pocket {
        for post in &layout.posts {
            inside_pocket(
                pocket,
                post.center,
                post.diameter / 2.0,
                "an alignment post",
            )?;
        }
    }
    if let Some(joint) = &layout.joint {
        for post in &layout.posts {
            for f in &joint.fasteners {
                // the screw head stands above the top plate, the same side as the post
                let need = SCREW_HEAD_DIAMETER / 2.0 + post.diameter / 2.0;
                if f.center.distance(post.center) < need {
                    return Err(Error::Geometry(format!(
                        "an alignment post at ({:.2}, {:.2}) mm is too close to the M3 screw at \
                         ({:.2}, {:.2}) mm used by --split (needs {need:.2} mm); increase --margin",
                        post.center.x, post.center.y, f.center.x, f.center.y
                    )));
                }
            }
        }
    }
    for (i, a) in layout.posts.iter().enumerate() {
        for b in layout.posts.iter().skip(i + 1) {
            if a.center.distance(b.center) < f64::midpoint(a.diameter, b.diameter) + mech.min_wall {
                return Err(Error::Geometry("two alignment posts overlap".into()));
            }
        }
    }
    Ok(())
}

/// `compact`: no two shoulders may merge, and no pogo bore may cut into a shoulder (the barrel
/// runs past it in the cavity). The bore keeps `--min-wall` from the shoulder.
fn check_stepped_pillars(layout: &Layout, probes: &[Probe], mech: &Mechanical) -> Result<()> {
    // mounting holes are named in board coordinates, as `ict-jig-gen holes` lists them
    let board_xy = |p: Point| (p.x - layout.offset.x, p.y - layout.offset.y);
    for (i, a) in layout.stepped_pillars.iter().enumerate() {
        for b in layout.stepped_pillars.iter().skip(i + 1) {
            let need = f64::midpoint(a.shoulder_diameter, b.shoulder_diameter) + mech.min_wall;
            if a.center.distance(b.center) < need {
                let ((ax, ay), (bx, by)) = (board_xy(a.center), board_xy(b.center));
                return Err(Error::Geometry(format!(
                    "--compact: the pillars on the mounting holes at ({ax:.2}, {ay:.2}) and \
                     ({bx:.2}, {by:.2}) mm would merge (their shoulders need {need:.2} mm between \
                     centres); drop --compact"
                )));
            }
        }
    }
    let hole_r = layout.hole_diameter / 2.0;
    for (a, pa) in layout.holes.iter().zip(probes) {
        for s in &layout.stepped_pillars {
            let need = hole_r + s.shoulder_diameter / 2.0 + mech.min_wall;
            let d = a.distance(s.center);
            if d < need {
                let (x, y) = board_xy(s.center);
                return Err(Error::Geometry(format!(
                    "pin {} ({}) collides with the --compact pillar on the mounting hole at \
                     ({x:.2}, {y:.2}) mm: they are {d:.2} mm apart, but the {:.2} mm shoulder and \
                     the {:.2} mm bore need {need:.2} mm; choose another test point or drop --compact",
                    pa.pin, pa.label, s.shoulder_diameter, layout.hole_diameter
                )));
            }
        }
    }
    Ok(())
}

fn inside_pocket(pocket: &Rect, p: Point, radius: f64, what: &str) -> Result<()> {
    let room = (p.x - pocket.min.x)
        .min(pocket.max.x - p.x)
        .min(p.y - pocket.min.y)
        .min(pocket.max.y - p.y);
    if room < radius + LIP_FEATURE_GAP {
        return Err(Error::Geometry(format!(
            "{what} sits too close to the board edge ({room:.2} mm from the registration lip); use \
             --alignment posts or none"
        )));
    }
    Ok(())
}

fn stack_up_warning(mech: &Mechanical, warnings: &mut Vec<String>) {
    let stack = 2.0 * mech.plate_thickness + mech.standoff_height;
    let reach = mech.pogo.length - stack;
    if reach < mech.pogo.stroke {
        warnings.push(format!(
            "{} is {:.2} mm long but the plate stack is {stack:.2} mm: with the tail flush with the \
             bottom, the plunger rises only {reach:.2} mm above the top plate (needs the {:.2} mm \
             stroke). Reduce --standoff-height or --plate-thickness",
            mech.pogo.name, mech.pogo.length, mech.pogo.stroke
        ));
    }
}

/// Polygon rings shared by the single and split builders.
struct Rings {
    n: usize,
    /// Scale that makes an n-gon circumscribe the circle it approximates.
    circumscribe: f64,
    footprint: Vec<P2>,
    holes: Vec<Vec<P2>>,
    pillars: Vec<Vec<P2>>,
    posts: Vec<Vec<P2>>,
}

impl Rings {
    fn new(l: &Layout, n: usize) -> Self {
        // Holes are circumscribed about the nominal circle so the printed bore is never
        // narrower than the barrel; posts are inscribed so they never exceed the mounting hole.
        #[allow(clippy::cast_precision_loss)]
        let circumscribe = 1.0 / (std::f64::consts::PI / n as f64).cos();
        Self {
            n,
            circumscribe,
            footprint: rect([0.0, 0.0], [l.width, l.depth]),
            holes: l
                .holes
                .iter()
                .map(|h| circle([h.x, h.y], l.hole_diameter / 2.0 * circumscribe, n))
                .collect(),
            pillars: l
                .pillars
                .iter()
                .map(Rect::ring)
                .chain(
                    l.stepped_pillars
                        .iter()
                        .map(|s| circle([s.center.x, s.center.y], s.shoulder_diameter / 2.0, n)),
                )
                .collect(),
            posts: l
                .posts
                .iter()
                .map(|p| circle([p.center.x, p.center.y], p.diameter / 2.0, n))
                .collect(),
        }
    }

    /// The footprint with every pogo bore and the `extra` rings cut out.
    fn plate(&self, extra: &[Vec<P2>]) -> Region {
        Region {
            outer: self.footprint.clone(),
            holes: self.holes.iter().chain(extra).cloned().collect(),
        }
    }

    /// Outer wall and bore walls of a plate between `z0` and `z1`.
    fn plate_walls(&self, solid: &mut Mesh, extra: &[Vec<P2>], z0: f64, z1: f64) {
        solid.wall(&self.footprint, z0, z1, true);
        for h in self.holes.iter().chain(extra) {
            solid.wall(h, z0, z1, false);
        }
    }
}

fn assemble(l: &Layout, mech: &Mechanical) -> Result<Solids> {
    let r = Rings::new(l, mech.circle_segments);
    match (&l.joint, mech.split) {
        (Some(joint), _) => Ok(Solids::Split {
            bottom: assemble_bottom(l, &r, joint)?,
            top: assemble_top(l, mech, &r, joint)?,
        }),
        (None, true) => Ok(Solids::Split {
            bottom: assemble_pinned_bottom(l, mech, &r)?,
            top: assemble_pinned_top(l, &r)?,
        }),
        (None, false) => Ok(Solids::Single(assemble_single(l, mech, &r)?)),
    }
}

fn assemble_single(l: &Layout, mech: &Mechanical, r: &Rings) -> Result<Mesh> {
    let (z1, z2, z3) = (
        l.z_bottom_plate_top,
        l.z_top_plate_bottom,
        l.z_top_plate_top,
    );
    let mut solid = Mesh::new();
    // bottom guide plate: z0..z1
    solid.cap(&r.plate(&[]), 0.0, false)?;
    r.plate_walls(&mut solid, &[], 0.0, z1);
    // cavity: floor, ceiling and the pillars joining them
    solid.cap(&r.plate(&r.pillars), z1, true)?;
    solid.cap(&r.plate(&r.pillars), z2, false)?;
    for p in &r.pillars {
        solid.wall(p, z1, z2, true);
    }
    // top guide plate: z2..z3
    r.plate_walls(&mut solid, &[], z2, z3);
    add_deck(&mut solid, l, mech, r, z3, &[])?;
    Ok(solid)
}

/// Bottom half, printed as it stands: plate on the bed, pillars (and bosses) rising from it.
fn assemble_bottom(l: &Layout, r: &Rings, joint: &Joint) -> Result<Mesh> {
    let (z1, z2) = (l.z_bottom_plate_top, l.z_top_plate_bottom);
    let boss_top = z2 + joint.boss_height;
    if let Some(f) = joint
        .fasteners
        .iter()
        .find(|f| !l.pillars.iter().any(|p| p.contains(f.center)))
    {
        return Err(Error::Geometry(format!(
            "internal error: the M3 fastener at ({:.2}, {:.2}) mm is not on a support pillar",
            f.center.x, f.center.y
        )));
    }
    let mut solid = Mesh::new();
    solid.cap(&r.plate(&[]), 0.0, false)?;
    r.plate_walls(&mut solid, &[], 0.0, z1);
    solid.cap(&r.plate(&r.pillars), z1, true)?;
    for (pillar, ring) in l.pillars.iter().zip(&r.pillars) {
        solid.wall(ring, z1, z2, true);
        let Some(f) = joint.fasteners.iter().find(|f| pillar.contains(f.center)) else {
            solid.cap(
                &Region {
                    outer: ring.clone(),
                    holes: vec![],
                },
                z2,
                true,
            )?;
            continue;
        };
        let c = [f.center.x, f.center.y];
        let boss = circle(c, f.boss_diameter / 2.0, r.n);
        let pilot = circle(c, SCREW_PILOT_DIAMETER / 2.0 * r.circumscribe, r.n);
        // pillar top (ring around the boss), boss wall and top, then the blind pilot hole
        solid.cap(
            &Region {
                outer: ring.clone(),
                holes: vec![boss.clone()],
            },
            z2,
            true,
        )?;
        solid.wall(&boss, z2, boss_top, true);
        solid.cap(
            &Region {
                outer: boss,
                holes: vec![pilot.clone()],
            },
            boss_top,
            true,
        )?;
        solid.wall(&pilot, z1, boss_top, false);
        solid.cap(
            &Region {
                outer: pilot,
                holes: vec![],
            },
            z1,
            true,
        )?;
    }
    Ok(solid)
}

/// Top half in its print orientation: the plate's underside (which meets the pillars) on the
/// bed, posts / lip on top. That is the assembled orientation shifted down by `z2`.
fn assemble_top(l: &Layout, mech: &Mechanical, r: &Rings, joint: &Joint) -> Result<Mesh> {
    let thickness = l.z_top_plate_top - l.z_top_plate_bottom;
    let sockets: Vec<Vec<P2>> = joint
        .fasteners
        .iter()
        .map(|f| {
            let radius = joint.socket_diameter(f) / 2.0 * r.circumscribe;
            circle([f.center.x, f.center.y], radius, r.n)
        })
        .collect();
    let mut solid = Mesh::new();
    solid.cap(&r.plate(&sockets), 0.0, false)?;
    r.plate_walls(&mut solid, &sockets, 0.0, thickness);
    add_deck(&mut solid, l, mech, r, thickness, &sockets)?;
    Ok(solid)
}

/// Compact split, bottom half, printed as it stands: the plate on the bed and a stepped pillar
/// on every mounting hole, its pin running on up through where the top plate will sit and
/// ending as the alignment post. Every face above the bed looks up or sideways.
fn assemble_pinned_bottom(l: &Layout, mech: &Mechanical, r: &Rings) -> Result<Mesh> {
    if l.stepped_pillars.is_empty() {
        return Err(Error::Geometry(
            "internal error: a split cassette without screws needs compact pillars".into(),
        ));
    }
    let (z1, z2) = (l.z_bottom_plate_top, l.z_top_plate_bottom);
    let post_top = l.z_top_plate_top + mech.pcb_thickness + POST_PROTRUSION;
    let shoulders: Vec<Vec<P2>> = l
        .stepped_pillars
        .iter()
        .map(|s| circle([s.center.x, s.center.y], s.shoulder_diameter / 2.0, r.n))
        .collect();
    let mut solid = Mesh::new();
    solid.cap(&r.plate(&[]), 0.0, false)?;
    r.plate_walls(&mut solid, &[], 0.0, z1);
    solid.cap(&r.plate(&shoulders), z1, true)?;
    for (s, shoulder) in l.stepped_pillars.iter().zip(&shoulders) {
        let pin = circle([s.center.x, s.center.y], s.pin_diameter / 2.0, r.n);
        solid.wall(shoulder, z1, z2, true);
        // the ledge the top plate rests on
        solid.cap(
            &Region {
                outer: shoulder.clone(),
                holes: vec![pin.clone()],
            },
            z2,
            true,
        )?;
        add_post(&mut solid, s.center, s.pin_diameter, &pin, z2, post_top)?;
    }
    Ok(solid)
}

/// Compact split, top half: a flat plate with the pogo bores and a socket for every pin. It is
/// symmetric through its thickness, so it prints and assembles the same way up.
fn assemble_pinned_top(l: &Layout, r: &Rings) -> Result<Mesh> {
    let thickness = l.z_top_plate_top - l.z_top_plate_bottom;
    let sockets: Vec<Vec<P2>> = l
        .stepped_pillars
        .iter()
        .map(|s| {
            let radius = s.socket_diameter / 2.0 * r.circumscribe;
            circle([s.center.x, s.center.y], radius, r.n)
        })
        .collect();
    let plate = r.plate(&sockets);
    let mut solid = Mesh::new();
    solid.cap(&plate, 0.0, false)?;
    r.plate_walls(&mut solid, &sockets, 0.0, thickness);
    solid.cap(&plate, thickness, true)?;
    Ok(solid)
}

/// Everything on the top plate's upper face at `z3`: the board seat, registration lip and
/// alignment posts. `sockets` are extra through-holes in the plate outside the seat.
fn add_deck(
    solid: &mut Mesh,
    l: &Layout,
    mech: &Mechanical,
    r: &Rings,
    z3: f64,
    sockets: &[Vec<P2>],
) -> Result<()> {
    if let Some(pocket) = &l.pocket {
        let inner = pocket.ring();
        let outer = rect(
            [pocket.min.x - LIP_WALL, pocket.min.y - LIP_WALL],
            [pocket.max.x + LIP_WALL, pocket.max.y + LIP_WALL],
        );
        let lip_top = z3 + mech.lip_height;
        solid.cap(
            &Region {
                outer: r.footprint.clone(),
                holes: std::iter::once(outer.clone())
                    .chain(sockets.iter().cloned())
                    .collect(),
            },
            z3,
            true,
        )?;
        let seat = Region {
            outer: inner.clone(),
            holes: r.holes.iter().chain(&r.posts).cloned().collect(),
        };
        solid.cap(&seat, z3, true)?;
        solid.wall(&outer, z3, lip_top, true);
        solid.wall(&inner, z3, lip_top, false);
        solid.cap(
            &Region {
                outer,
                holes: vec![inner],
            },
            lip_top,
            true,
        )?;
    } else {
        let extra: Vec<Vec<P2>> = sockets.iter().chain(&r.posts).cloned().collect();
        solid.cap(&r.plate(&extra), z3, true)?;
    }
    add_posts(solid, l, mech, &r.posts, z3)
}

fn add_posts(
    solid: &mut Mesh,
    l: &Layout,
    mech: &Mechanical,
    rings: &[Vec<P2>],
    z3: f64,
) -> Result<()> {
    let top = z3 + mech.pcb_thickness + POST_PROTRUSION;
    for (post, ring) in l.posts.iter().zip(rings) {
        add_post(solid, post.center, post.diameter, ring, z3, top)?;
    }
    Ok(())
}

/// A round shaft on `ring` from `base` up to a tapered tip that ends at `top`.
fn add_post(
    solid: &mut Mesh,
    center: Point,
    diameter: f64,
    ring: &[P2],
    base: f64,
    top: f64,
) -> Result<()> {
    let r = diameter / 2.0;
    let tip_r = (r - 0.5).max(r * 0.5);
    let tip = circle([center.x, center.y], tip_r, ring.len());
    let shaft_top = top - POST_TIP_HEIGHT;
    solid.wall(ring, base, shaft_top, true);
    solid.frustum(ring, shaft_top, &tip, top);
    solid.cap(
        &Region {
            outer: tip,
            holes: vec![],
        },
        top,
        true,
    )
}
