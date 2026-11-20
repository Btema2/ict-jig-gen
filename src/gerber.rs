//! Minimal RS-274X reader for the board-outline layer.
//!
//! Only the geometry needed to find the board extents is interpreted: linear and circular
//! draws (G01/G02/G03, single and multi quadrant), flashes, units and the coordinate format.
//! Aperture sizes, macros, polarity and everything else are skipped, because the outline's
//! centre line defines the board edge.

use crate::error::{Error, Result};
use crate::geom::{BBox, Point};
use crate::numfmt::{CoordFormat, Unit, ZeroOmit, tokens};

/// Result of parsing an outline layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    /// Bounding box of all outline geometry, in millimetres.
    pub bbox: BBox,
}

/// Largest plausible board dimension; anything bigger is treated as a corrupt file.
const MAX_BOARD_MM: f64 = 2000.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Interp {
    Linear,
    Clockwise,
    CounterClockwise,
}

struct State<'a> {
    file: &'a str,
    unit: Option<Unit>,
    format: Option<CoordFormat>,
    interp: Interp,
    multi_quadrant: bool,
    last_op: u32,
    pos: Point,
    bbox: Option<BBox>,
}

/// Parse a Gerber outline layer and return its extents in millimetres.
pub fn parse_outline(text: &str, file: &str) -> Result<Outline> {
    let mut st = State {
        file,
        unit: None,
        format: None,
        interp: Interp::Linear,
        multi_quadrant: true,
        last_op: 2,
        pos: Point::default(),
        bbox: None,
    };
    for (line, stmt) in statements(text, file)? {
        match stmt {
            Stmt::Extended(body) => st.extended(&body, line)?,
            Stmt::Word(word) => {
                if st.word(&word, line)? == Flow::End {
                    break;
                }
            }
        }
    }
    let bbox = st
        .bbox
        .ok_or_else(|| Error::parse(file, "outline layer contains no geometry"))?;
    if bbox.width() < 1e-3 || bbox.height() < 1e-3 {
        return Err(Error::parse(file, "outline has zero width or height"));
    }
    if bbox.width() > MAX_BOARD_MM || bbox.height() > MAX_BOARD_MM {
        return Err(Error::parse(
            file,
            format!(
                "outline is {:.0} x {:.0} mm, larger than the {MAX_BOARD_MM} mm sanity limit \
                 (wrong units?)",
                bbox.width(),
                bbox.height()
            ),
        ));
    }
    Ok(Outline { bbox })
}

enum Stmt {
    /// Contents of a `%...%` block (several `*`-terminated parts).
    Extended(String),
    /// One `*`-terminated function block.
    Word(String),
}

#[derive(PartialEq, Eq)]
enum Flow {
    Continue,
    End,
}

/// Split the file into statements, remembering each one's starting line.
fn statements(text: &str, file: &str) -> Result<Vec<(usize, Stmt)>> {
    let mut out = Vec::new();
    let mut rest = text;
    let mut line = 1usize;
    loop {
        let trimmed = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
        line += rest[..rest.len() - trimmed.len()].matches('\n').count();
        rest = trimmed;
        if rest.is_empty() {
            return Ok(out);
        }
        if let Some(after) = rest.strip_prefix('%') {
            let end = after
                .find('%')
                .ok_or_else(|| Error::parse_at(file, line, "unterminated %...% block"))?;
            out.push((line, Stmt::Extended(after[..end].to_owned())));
            line += after[..=end].matches('\n').count();
            rest = &after[end + 1..];
        } else {
            let end = rest
                .find('*')
                .ok_or_else(|| Error::parse_at(file, line, "missing '*' block terminator"))?;
            out.push((line, Stmt::Word(rest[..end].to_owned())));
            line += rest[..=end].matches('\n').count();
            rest = &rest[end + 1..];
        }
    }
}

impl State<'_> {
    fn err(&self, line: usize, message: impl Into<String>) -> Error {
        Error::parse_at(self.file, line, message)
    }

    fn extended(&mut self, body: &str, line: usize) -> Result<()> {
        let body = body.trim();
        if body.starts_with("AM") {
            return Ok(()); // macro definitions span several parts; irrelevant for extents
        }
        for part in body.split('*').map(str::trim).filter(|p| !p.is_empty()) {
            if let Some(spec) = part.strip_prefix("FS") {
                self.format = Some(self.parse_format(spec, line)?);
            } else if let Some(unit) = part.strip_prefix("MO") {
                self.unit = Some(match unit.trim() {
                    "MM" => Unit::Mm,
                    "IN" => Unit::Inch,
                    other => return Err(self.err(line, format!("unknown unit %MO{other}%"))),
                });
            }
        }
        Ok(())
    }

    /// `LAX24Y24` / `TAX36Y36`: zero suppression, absolute/incremental, X and Y digit counts.
    fn parse_format(&self, spec: &str, line: usize) -> Result<CoordFormat> {
        let bad = || self.err(line, format!("unsupported coordinate format %FS{spec}%"));
        let mut chars = spec.chars();
        let omit = match chars.next() {
            Some('L' | 'D') => ZeroOmit::Leading,
            Some('T') => ZeroOmit::Trailing,
            _ => return Err(bad()),
        };
        if chars.next() != Some('A') {
            return Err(self.err(line, "incremental coordinates (FS..I) are not supported"));
        }
        let rest: String = chars.collect();
        let (x, y) = rest
            .strip_prefix('X')
            .and_then(|r| r.split_once('Y'))
            .ok_or_else(bad)?;
        let digits = |s: &str| -> Option<(u8, u8)> {
            let mut it = s.bytes().map(|b| b.checked_sub(b'0').filter(|d| *d <= 9));
            let (i, d) = (it.next()??, it.next()??);
            it.next().is_none().then_some((i, d))
        };
        let (xf, yf) = (digits(x).ok_or_else(bad)?, digits(y).ok_or_else(bad)?);
        if xf != yf {
            return Err(self.err(
                line,
                "different X and Y coordinate formats are not supported",
            ));
        }
        Ok(CoordFormat {
            int_digits: xf.0,
            dec_digits: xf.1,
            omit,
        })
    }

    fn word(&mut self, word: &str, line: usize) -> Result<Flow> {
        let word = word.trim();
        if word.starts_with("G04") || word.is_empty() {
            return Ok(Flow::Continue); // comment
        }
        let mut coords: [Option<&str>; 4] = [None; 4]; // X, Y, I, J
        let mut op = None;
        for (letter, value) in tokens(word).ok_or_else(|| self.err(line, "malformed block"))? {
            let number = value.parse::<u32>().ok();
            match letter {
                'X' => coords[0] = Some(value),
                'Y' => coords[1] = Some(value),
                'I' => coords[2] = Some(value),
                'J' => coords[3] = Some(value),
                'G' => self.g_code(number, line)?,
                'D' => match number {
                    Some(n @ 1..=3) => op = Some(n),
                    Some(_) => {} // aperture selection
                    None => return Err(self.err(line, format!("bad D-code {value:?}"))),
                },
                'M' if matches!(number, Some(0..=2)) => return Ok(Flow::End),
                _ => {}
            }
        }
        if coords.iter().all(Option::is_none) && op.is_none() {
            return Ok(Flow::Continue);
        }
        let op = op.unwrap_or(self.last_op);
        self.last_op = op;
        self.operate(op, &coords, line)?;
        Ok(Flow::Continue)
    }

    fn g_code(&mut self, number: Option<u32>, line: usize) -> Result<()> {
        match number {
            Some(1) => self.interp = Interp::Linear,
            Some(2) => self.interp = Interp::Clockwise,
            Some(3) => self.interp = Interp::CounterClockwise,
            Some(70) => self.unit = Some(Unit::Inch),
            Some(71) => self.unit = Some(Unit::Mm),
            Some(74) => self.multi_quadrant = false,
            Some(75) => self.multi_quadrant = true,
            Some(91) => return Err(self.err(line, "incremental coordinates (G91) not supported")),
            _ => {} // G36/G37 regions, G54, G90, ... need no special handling
        }
        Ok(())
    }

    fn operate(&mut self, op: u32, coords: &[Option<&str>; 4], line: usize) -> Result<()> {
        let (Some(unit), Some(format)) = (self.unit, self.format) else {
            return Err(self.err(line, "coordinates used before units (%MO) / format (%FS)"));
        };
        let decode = |raw: Option<&str>, fallback: f64| -> Result<f64> {
            raw.map_or(Ok(fallback), |r| {
                format
                    .parse(r)
                    .map(|v| unit.to_mm(v))
                    .ok_or_else(|| self.err(line, format!("bad coordinate {r:?}")))
            })
        };
        let target = Point::new(
            decode(coords[0], self.pos.x)?,
            decode(coords[1], self.pos.y)?,
        );
        let (i, j) = (decode(coords[2], 0.0)?, decode(coords[3], 0.0)?);
        let start = self.pos;
        match op {
            1 => {
                self.grow(start);
                self.grow(target);
                if self.interp != Interp::Linear {
                    let cw = self.interp == Interp::Clockwise;
                    self.grow_arc(start, target, (i, j), cw, line)?;
                }
            }
            3 => self.grow(target),
            _ => {}
        }
        self.pos = target;
        Ok(())
    }

    fn grow(&mut self, p: Point) {
        match &mut self.bbox {
            Some(b) => b.include(p),
            None => self.bbox = Some(BBox::around(p)),
        }
    }

    fn grow_arc(
        &mut self,
        start: Point,
        end: Point,
        (i, j): (f64, f64),
        clockwise: bool,
        line: usize,
    ) -> Result<()> {
        let center = if self.multi_quadrant {
            Point::new(start.x + i, start.y + j)
        } else {
            single_quadrant_center(start, end, i.abs(), j.abs(), clockwise)
        };
        let radius = center.distance(start);
        if (center.distance(end) - radius).abs() > 0.02_f64.max(0.01 * radius) {
            return Err(self.err(line, "arc end points are not equidistant from its centre"));
        }
        let a0 = (start.y - center.y).atan2(start.x - center.x);
        let a1 = (end.y - center.y).atan2(end.x - center.x);
        let tau = std::f64::consts::TAU;
        let ccw_sweep = (a1 - a0).rem_euclid(tau);
        let full = start.distance(end) < 1e-9;
        let sweep = if full {
            tau
        } else if clockwise {
            tau - ccw_sweep
        } else {
            ccw_sweep
        };
        for quadrant in 0..4u8 {
            let theta = f64::from(quadrant) * std::f64::consts::FRAC_PI_2;
            let delta = if clockwise { a0 - theta } else { theta - a0 }.rem_euclid(tau);
            if delta <= sweep {
                self.grow(Point::new(
                    center.x + radius * theta.cos(),
                    center.y + radius * theta.sin(),
                ));
            }
        }
        Ok(())
    }
}

/// Centre of a G74 (single-quadrant) arc: I/J are unsigned, so try the four sign choices and
/// keep the one whose radii agree best.
fn single_quadrant_center(start: Point, end: Point, i: f64, j: f64, clockwise: bool) -> Point {
    let mut best = (f64::INFINITY, Point::new(start.x + i, start.y + j));
    for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        let c = Point::new(start.x + sx * i, start.y + sy * j);
        let mismatch = (c.distance(start) - c.distance(end)).abs();
        let a0 = (start.y - c.y).atan2(start.x - c.x);
        let a1 = (end.y - c.y).atan2(end.x - c.x);
        let ccw = (a1 - a0).rem_euclid(std::f64::consts::TAU);
        let sweep = if clockwise {
            std::f64::consts::TAU - ccw
        } else {
            ccw
        };
        // a quadrant arc never exceeds 90 degrees; penalise candidates that do
        let penalty = if sweep > std::f64::consts::FRAC_PI_2 + 1e-6 {
            1e3
        } else {
            0.0
        };
        if mismatch + penalty < best.0 {
            best = (mismatch + penalty, c);
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox(text: &str) -> BBox {
        parse_outline(text, "t.GKO").unwrap().bbox
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    const RECT_MM: &str = "G04 test*\n%FSLAX33Y33*%\n%MOMM*%\n%ADD10C,0.100*%\nG01*\nD10*\n\
        X10000Y20000D02*\nX70000D01*\nY60000D01*\nX10000D01*\nY20000D01*\nM02*\n";

    #[test]
    fn metric_rectangle() {
        let b = bbox(RECT_MM);
        assert!(close(b.min.x, 10.0) && close(b.min.y, 20.0));
        assert!(close(b.max.x, 70.0) && close(b.max.y, 60.0));
    }

    #[test]
    fn imperial_four_five_format() {
        let b =
            bbox("%FSLAX24Y24*%\n%MOIN*%\nD10*\nX10000Y10000D02*\nX30000D01*\nY20000D01*\nM02*");
        assert!(close(b.min.x, 25.4) && close(b.max.x, 76.2));
        assert!(close(b.max.y, 50.8));
    }

    #[test]
    fn single_line_and_cr_lf() {
        let text = RECT_MM.replace('\n', "\r\n");
        assert_eq!(bbox(&text), bbox(RECT_MM));
        let compact = "%FSLAX33Y33*%%MOMM*%D10*X0Y0D02*X5000D01*Y5000D01*M02*";
        assert!(close(bbox(compact).width(), 5.0));
    }

    #[test]
    fn multi_quadrant_arc_extends_bbox() {
        // Half circle (radius 10) from (10,0) to (-10,0) counter-clockwise through (0,10).
        let b = bbox("%FSLAX33Y33*%%MOMM*%G75*G03*D10*X10000Y0D02*X-10000Y0I-10000J0D01*M02*");
        assert!(close(b.max.y, 10.0), "{b:?}");
        assert!(close(b.min.y, 0.0));
        assert!(close(b.min.x, -10.0) && close(b.max.x, 10.0));
    }

    #[test]
    fn clockwise_arc_goes_the_other_way() {
        let b = bbox("%FSLAX33Y33*%%MOMM*%G75*G02*D10*X10000Y0D02*X-10000Y0I-10000J0D01*M02*");
        assert!(close(b.min.y, -10.0), "{b:?}");
        assert!(close(b.max.y, 0.0));
    }

    #[test]
    fn single_quadrant_arc() {
        // Quarter arc (r=5) from (5,0) to (0,5) CCW about the origin; G74 uses unsigned I/J.
        let b = bbox("%FSLAX33Y33*%%MOMM*%G74*G03*D10*X5000Y0D02*X0Y5000I5000J0D01*M02*");
        assert!(close(b.max.x, 5.0) && close(b.max.y, 5.0), "{b:?}");
        assert!(close(b.min.x, 0.0) && close(b.min.y, 0.0));
    }

    #[test]
    fn trailing_zero_format() {
        let b = bbox("%FSTAX33Y33*%%MOMM*%D10*X1Y1D02*X5D01*Y4D01*M02*");
        // trailing zeros omitted: "1" is padded to "100000" and read as 3:3, i.e. 100.000
        assert!(close(b.min.x, 100.0) && close(b.max.x, 500.0), "{b:?}");
    }

    #[test]
    fn errors_are_reported_not_panicked() {
        for (text, needle) in [
            ("", "no geometry"),
            ("%FSLAX33Y33*%%MOMM*%D10*X1Y1D02*", "no geometry"),
            ("D10*X1Y1D02*X2D01*", "before units"),
            ("%FSLAX33Y33*%%MOMM*%X10", "terminator"),
            ("%FSLAX33Y33*%%MOMM*", "unterminated"),
            ("%FSLIX33Y33*%%MOMM*%D10*X1D02*", "incremental"),
            (
                "%FSLAX33Y33*%%MOMM*%D10*X3000000Y3000000D02*X0Y0D01*",
                "sanity",
            ),
            ("%FSLAX33Y33*%%MOMM*%D10*Xabc*", "bad coordinate"),
            ("%FSLAX33Y33*%%MOXX*%", "unknown unit"),
        ] {
            let e = parse_outline(text, "bad.GKO").unwrap_err().to_string();
            assert!(e.contains(needle), "{text:?} -> {e}");
            assert!(e.contains("bad.GKO"));
        }
    }

    #[test]
    fn zero_height_outline_is_rejected() {
        let e = parse_outline("%FSLAX33Y33*%%MOMM*%D10*X0Y0D02*X5000D01*M02*", "f").unwrap_err();
        assert!(e.to_string().contains("zero width or height"));
    }
}
