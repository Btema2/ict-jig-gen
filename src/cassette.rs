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

/// An alignment post.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Post {
    /// Centre in cassette coordinates.
    pub center: Point,
    /// Shaft diameter.
    pub diameter: f64,
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
    /// Support pillars.
    pub pillars: Vec<Rect>,
}

/// Generated cassette.
#[derive(Debug, Clone)]
pub struct Cassette {
    /// The solid.
    pub mesh: Mesh,
    /// Where everything is.
    pub layout: Layout,
    /// Non-fatal observations.
    pub warnings: Vec<String>,
}

/// Build the cassette for the given probes (one hole per probe, in order).
pub fn build(board: &Board, probes: &[Probe], mech: &Mechanical) -> Result<Cassette> {
    if probes.is_empty() {
        return Err(Error::Geometry("no probe points selected".into()));
    }
    let mut warnings = Vec::new();
    let offset = Point::new(
        mech.margin - board.bbox.min.x,
        mech.margin - board.bbox.min.y,
    );
    let to_cassette = |p: Point| Point::new(p.x + offset.x, p.y + offset.y);
    let (width, depth) = (
        board.bbox.width() + 2.0 * mech.margin,
        board.bbox.height() + 2.0 * mech.margin,
    );

    let holes: Vec<Point> = probes.iter().map(|p| to_cassette(p.hole.pos)).collect();
    let (posts, use_lip) = plan_alignment(board, mech, &to_cassette, &mut warnings)?;
    let pocket = use_lip.then(|| {
        let min = Point::new(
            mech.margin - POCKET_CLEARANCE,
            mech.margin - POCKET_CLEARANCE,
        );
        let max = Point::new(
            width - mech.margin + POCKET_CLEARANCE,
            depth - mech.margin + POCKET_CLEARANCE,
        );
        Rect { min, max }
    });
    let pillars = plan_pillars(width, depth);

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
        offset,
        z_bottom_plate_top: z1,
        z_top_plate_bottom: z2,
        z_top_plate_top: z3,
        hole_diameter: mech.hole_diameter(),
        holes,
        posts,
        pocket,
        pillars,
    };
    check_clearances(&layout, probes, mech)?;
    stack_up_warning(mech, &mut warnings);

    let solid = assemble(&layout, mech)?;
    Ok(Cassette {
        mesh: solid,
        layout,
        warnings,
    })
}

fn plan_alignment(
    board: &Board,
    mech: &Mechanical,
    to_cassette: &impl Fn(Point) -> Point,
    warnings: &mut Vec<String>,
) -> Result<(Vec<Post>, bool)> {
    let eligible: Vec<Post> = board
        .mounting
        .iter()
        .filter(|h| h.diameter >= MIN_POST_HOLE_DIAMETER)
        .map(|h| Post {
            center: to_cassette(h.pos),
            diameter: h.diameter - mech.clearance,
        })
        .collect();
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
    for (i, a) in layout.posts.iter().enumerate() {
        for b in layout.posts.iter().skip(i + 1) {
            if a.center.distance(b.center) < f64::midpoint(a.diameter, b.diameter) + mech.min_wall {
                return Err(Error::Geometry("two alignment posts overlap".into()));
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

fn assemble(l: &Layout, mech: &Mechanical) -> Result<Mesh> {
    let n = mech.circle_segments;
    // Holes are circumscribed about the nominal circle so the printed bore is never narrower
    // than the barrel; posts are inscribed so they never exceed the mounting hole.
    #[allow(clippy::cast_precision_loss)]
    let circumscribe = 1.0 / (std::f64::consts::PI / n as f64).cos();
    let hole_rings: Vec<Vec<P2>> = l
        .holes
        .iter()
        .map(|h| circle([h.x, h.y], l.hole_diameter / 2.0 * circumscribe, n))
        .collect();
    let pillar_rings: Vec<Vec<P2>> = l.pillars.iter().map(Rect::ring).collect();
    let footprint = rect([0.0, 0.0], [l.width, l.depth]);
    let (z1, z2, z3) = (
        l.z_bottom_plate_top,
        l.z_top_plate_bottom,
        l.z_top_plate_top,
    );

    let mut solid = Mesh::new();
    let with = |extra: &[Vec<P2>], also: &[Vec<P2>]| Region {
        outer: footprint.clone(),
        holes: hole_rings
            .iter()
            .chain(extra)
            .chain(also)
            .cloned()
            .collect(),
    };

    // bottom guide plate: z0..z1
    solid.cap(&with(&[], &[]), 0.0, false)?;
    solid.wall(&footprint, 0.0, z1, true);
    for h in &hole_rings {
        solid.wall(h, 0.0, z1, false);
    }
    // cavity: floor, ceiling and the pillars joining them
    solid.cap(&with(&pillar_rings, &[]), z1, true)?;
    solid.cap(&with(&pillar_rings, &[]), z2, false)?;
    for p in &pillar_rings {
        solid.wall(p, z1, z2, true);
    }
    // top guide plate: z2..z3
    solid.wall(&footprint, z2, z3, true);
    for h in &hole_rings {
        solid.wall(h, z2, z3, false);
    }

    let post_rings: Vec<Vec<P2>> = l
        .posts
        .iter()
        .map(|p| circle([p.center.x, p.center.y], p.diameter / 2.0, n))
        .collect();
    match &l.pocket {
        Some(pocket) => {
            let inner = pocket.ring();
            let outer = rect(
                [pocket.min.x - LIP_WALL, pocket.min.y - LIP_WALL],
                [pocket.max.x + LIP_WALL, pocket.max.y + LIP_WALL],
            );
            let lip_top = z3 + mech.lip_height;
            solid.cap(
                &Region {
                    outer: footprint.clone(),
                    holes: vec![outer.clone()],
                },
                z3,
                true,
            )?;
            let seat = Region {
                outer: inner.clone(),
                holes: hole_rings.iter().chain(&post_rings).cloned().collect(),
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
        }
        None => solid.cap(&with(&post_rings, &[]), z3, true)?,
    }
    add_posts(&mut solid, l, mech, &post_rings, z3)?;
    Ok(solid)
}

fn add_posts(
    solid: &mut Mesh,
    l: &Layout,
    mech: &Mechanical,
    rings: &[Vec<P2>],
    z3: f64,
) -> Result<()> {
    let height = mech.pcb_thickness + POST_PROTRUSION;
    let shaft_top = z3 + height - POST_TIP_HEIGHT;
    for (post, ring) in l.posts.iter().zip(rings) {
        let r = post.diameter / 2.0;
        let tip_r = (r - 0.5).max(r * 0.5);
        let tip = circle([post.center.x, post.center.y], tip_r, ring.len());
        solid.wall(ring, z3, shaft_top, true);
        solid.frustum(ring, shaft_top, &tip, z3 + height);
        solid.cap(
            &Region {
                outer: tip,
                holes: vec![],
            },
            z3 + height,
            true,
        )?;
    }
    Ok(())
}
