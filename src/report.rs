//! Human-readable reports: the wiring guide for the hardware builder and the hole listing.

use std::fmt::Write as _;

use crate::board::Board;
use crate::cassette::{
    Cassette, Joint, SCREW_HEAD_DIAMETER, SCREW_PILOT_DIAMETER, STL_BOTTOM_NAME, STL_TOP_NAME,
};
use crate::config::{Mechanical, Typ};
use crate::pinmap::{self, PinMap};

/// Which numbered wire goes into which hole of the printed cassette.
///
/// `cassette.layout.holes` must be in the same order as `map.probes`.
pub fn wiring_guide(
    board: &Board,
    map: &PinMap,
    cassette: &Cassette,
    mech: &Mechanical,
    warnings: &[String],
) -> String {
    let l = &cassette.layout;
    let mut out = String::new();
    // Writing to a String cannot fail, so the fmt::Result values are intentionally ignored.
    let _ = writeln!(out, "ICT cassette wiring guide");
    let _ = writeln!(out, "=========================");
    let _ = writeln!(
        out,
        "Board     : {}  ({:.2} x {:.2} mm)",
        board.source,
        board.bbox.width(),
        board.bbox.height()
    );
    let _ = writeln!(
        out,
        "Pogo pin  : {} (barrel {:.2} mm, hole {:.2} mm, length {:.2} mm, stroke {:.2} mm)",
        mech.pogo.name,
        mech.pogo.barrel_diameter,
        l.hole_diameter,
        mech.pogo.length,
        mech.pogo.stroke
    );
    let _ = writeln!(
        out,
        "Cassette  : {:.2} x {:.2} x {:.2} mm; plates {:.2} mm, gap {:.2} mm",
        l.width, l.depth, l.height, mech.plate_thickness, mech.standoff_height
    );
    if let Some(joint) = &l.joint {
        assembly_section(&mut out, joint);
    }
    if l.joint.is_some() {
        let _ = writeln!(out);
    }
    let _ = writeln!(
        out,
        "Coordinates: origin = lower-left corner of the cassette seen from above, +X right, +Y up."
    );
    let _ = writeln!(
        out,
        "The board lies component-side up; probes touch the underside pads.\n"
    );
    let _ = writeln!(
        out,
        "Wire        X (mm)    Y (mm)   Pad / label     Board X   Board Y"
    );
    let _ = writeln!(
        out,
        "----  ---------  --------  --------------  --------  --------"
    );
    for (probe, hole) in map.probes.iter().zip(&l.holes) {
        let _ = writeln!(
            out,
            "{:>4}  {:>9.2}  {:>8.2}  {:<14}  {:>8.2}  {:>8.2}",
            probe.pin, hole.x, hole.y, probe.label, probe.hole.pos.x, probe.hole.pos.y
        );
    }
    let _ = writeln!(out, "\nChannels in use: {} of 16", map.probes.len());
    if !map.elements.is_empty() {
        let _ = writeln!(out, "\nElements (lista_elementow.json)");
        let _ = writeln!(out, "Name              Pins     Typ");
        for e in &map.elements {
            let typ = if e.typ == Typ::Linia {
                "linia"
            } else {
                "element"
            };
            let _ = writeln!(
                out,
                "{:<16}  {:>2}-{:<2}    {typ}",
                e.name, e.pins[0], e.pins[1]
            );
        }
    }
    if !warnings.is_empty() {
        let _ = writeln!(out, "\nWarnings");
        for w in warnings {
            let _ = writeln!(out, "  - {w}");
        }
    }
    out
}

/// Print and assembly instructions for a `--split` cassette.
fn assembly_section(out: &mut String, joint: &Joint) {
    let screws = joint.fasteners.len();
    let screw = joint.screw_length.map_or_else(
        || format!("{screws}x M3 screws (no standard length fits these pillars)"),
        |len| format!("{screws}x M3 x {len} mm screws"),
    );
    let boss = |keyed: bool| {
        joint
            .fasteners
            .iter()
            .find(|f| f.keyed == keyed)
            .map_or(0.0, |f| f.boss_diameter)
    };
    let key = joint
        .fasteners
        .iter()
        .find(|f| f.keyed)
        .map_or_else(String::new, |k| {
            format!(" (X {:.1}, Y {:.1} mm)", k.center.x, k.center.y)
        });
    let _ = writeln!(
        out,
        "\nSplit assembly (--split): two parts, printed flat, no supports"
    );
    let _ = writeln!(
        out,
        "  {STL_BOTTOM_NAME}  bottom plate + pillars + locating bosses (bosses face up)"
    );
    let _ = writeln!(
        out,
        "  {STL_TOP_NAME}     top plate as it prints: underside on the bed, posts/lip up"
    );
    let _ = writeln!(
        out,
        "  Hardware: {screw}; pan, button or cap head, at most {SCREW_HEAD_DIAMETER:.1} mm across."
    );
    let _ = writeln!(
        out,
        "            No nuts: the screws cut their own thread in the {SCREW_PILOT_DIAMETER:.1} mm pilot holes."
    );
    let _ = writeln!(
        out,
        "  1. Print both files as exported: no supports, no rotating."
    );
    let _ = writeln!(
        out,
        "  2. Fit the pogo pins and wires into the bottom part (wire table below)."
    );
    let _ = writeln!(
        out,
        "  3. Lower the top part onto the pillars, its bores over the pogo barrels. The corner"
    );
    let _ = writeln!(
        out,
        "     bosses enter the sockets. The lower-left boss{key} is {:.2} mm, the others",
        boss(true)
    );
    let _ = writeln!(
        out,
        "     {:.2} mm, so the top only fits one way round.",
        boss(false)
    );
    let _ = writeln!(
        out,
        "  4. Drive one screw through the top plate at each corner into its boss. Snug, not tight."
    );
}

/// Table of every plated hole with its id, so users can pick probes (`H<id>`).
pub fn hole_listing(board: &Board) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Board {} : {:.2} x {:.2} mm, X {:.3}..{:.3}, Y {:.3}..{:.3}",
        board.source,
        board.bbox.width(),
        board.bbox.height(),
        board.bbox.min.x,
        board.bbox.max.x,
        board.bbox.min.y,
        board.bbox.max.y
    );
    let _ = writeln!(
        out,
        "\nPlated holes (probe candidates): {}",
        board.plated.len()
    );
    let _ = writeln!(out, "  ID        X (mm)     Y (mm)   Drill   Pad");
    for info in pinmap::catalog(board) {
        let pad = match (info.component, info.pad) {
            (Some(c), Some(p)) => format!("{}.{p}", board.components[c].designator),
            _ => "-".into(),
        };
        let _ = writeln!(
            out,
            "  H{:<3} {:>10.3} {:>10.3}  {:>5.2}   {pad}",
            info.id, info.hole.pos.x, info.hole.pos.y, info.hole.diameter
        );
    }
    let _ = writeln!(
        out,
        "\nMounting holes (alignment posts): {}",
        board.mounting.len()
    );
    for h in &board.mounting {
        let _ = writeln!(
            out,
            "       {:>10.3} {:>10.3}  {:>5.2}",
            h.pos.x, h.pos.y, h.diameter
        );
    }
    if !board.warnings.is_empty() {
        let _ = writeln!(out, "\nWarnings");
        for w in &board.warnings {
            let _ = writeln!(out, "  - {w}");
        }
    }
    out
}
