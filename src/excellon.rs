//! Excellon (NC drill) reader: holes with diameters, in millimetres.
//!
//! Handles metric and imperial files, `TZ`/`LZ` zero suppression, explicit decimal points and
//! the fixed-point precisions `EasyEDA` and other CAM tools emit (3:3, 4:2, 2:4, ...), declared
//! via `METRIC,000.000`-style headers or `;FILE_FORMAT=i:d` comments. Slots (`G85` or routed
//! paths) are not probe targets and are counted but skipped.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::geom::Point;
use crate::numfmt::{CoordFormat, Unit, ZeroOmit, tokens};

/// A drilled hole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hole {
    /// Centre, millimetres.
    pub pos: Point,
    /// Drill diameter, millimetres.
    pub diameter: f64,
}

/// Parsed drill file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DrillFile {
    /// Round holes in file order.
    pub holes: Vec<Hole>,
    /// Number of slots / routed paths that were ignored.
    pub skipped_slots: usize,
}

const MAX_COORD_MM: f64 = 5000.0;

struct State<'a> {
    file: &'a str,
    unit: Option<Unit>,
    digits: Option<(u8, u8)>,
    omit: ZeroOmit,
    tools: HashMap<u32, f64>,
    tool: Option<u32>,
    routing: bool,
    pos: Point,
    out: DrillFile,
}

/// Parse an Excellon drill file.
pub fn parse(text: &str, file: &str) -> Result<DrillFile> {
    let mut st = State {
        file,
        unit: None,
        digits: None,
        omit: ZeroOmit::Leading,
        tools: HashMap::new(),
        tool: None,
        routing: false,
        pos: Point::default(),
        out: DrillFile::default(),
    };
    for (idx, raw) in text.lines().enumerate() {
        let line = idx + 1;
        let l = raw.trim().trim_start_matches('\u{feff}');
        if l.is_empty() || l == "%" || l == "M48" || l == "M95" {
            continue;
        }
        if let Some(comment) = l.strip_prefix(';') {
            st.comment(comment);
        } else if l.starts_with("METRIC") || l.starts_with("INCH") {
            st.unit_line(l, line)?;
        } else if matches!(l, "M30" | "M00") {
            break;
        } else if l.starts_with("FMAT") || l.starts_with("VER") || l.starts_with("ICI") {
            // format / version / incremental-input switches carry nothing we need
        } else {
            st.statement(l, line)?;
        }
    }
    Ok(st.out)
}

impl State<'_> {
    fn err(&self, line: usize, message: impl Into<String>) -> Error {
        Error::parse_at(self.file, line, message)
    }

    /// `;FILE_FORMAT=3:3` is the only comment with semantic content.
    fn comment(&mut self, text: &str) {
        let Some(spec) = text.trim().strip_prefix("FILE_FORMAT=") else {
            return;
        };
        if let Some((i, d)) = spec.split_once(':')
            && let (Ok(i), Ok(d)) = (i.trim().parse::<u8>(), d.trim().parse::<u8>())
            && i + d > 0
            && i + d <= 12
        {
            self.digits = Some((i, d));
        }
    }

    /// `METRIC,TZ` / `INCH,LZ,00.0000` / `METRIC,000.000`.
    fn unit_line(&mut self, l: &str, line: usize) -> Result<()> {
        let mut parts = l.split(',').map(str::trim);
        self.unit = Some(match parts.next() {
            Some("METRIC") => Unit::Mm,
            Some("INCH") => Unit::Inch,
            other => return Err(self.err(line, format!("unknown unit {other:?}"))),
        });
        for part in parts {
            match part {
                "TZ" => self.omit = ZeroOmit::Leading,
                "LZ" => self.omit = ZeroOmit::Trailing,
                p if p.contains('.') && p.bytes().all(|b| b == b'0' || b == b'.') => {
                    if let Some((i, d)) = p.split_once('.')
                        && let (Ok(i), Ok(d)) = (u8::try_from(i.len()), u8::try_from(d.len()))
                    {
                        self.digits.get_or_insert((i, d));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn format(&self) -> CoordFormat {
        let (int_digits, dec_digits) = self.digits.unwrap_or(match self.unit {
            Some(Unit::Inch) => (2, 4),
            _ => (3, 3),
        });
        CoordFormat {
            int_digits,
            dec_digits,
            omit: self.omit,
        }
    }

    fn statement(&mut self, l: &str, line: usize) -> Result<()> {
        let toks = tokens(l).ok_or_else(|| self.err(line, format!("unrecognised line {l:?}")))?;
        let value = |letter: char| toks.iter().find(|(c, _)| *c == letter).map(|(_, v)| *v);
        let mut slot = false;
        for (letter, v) in &toks {
            let n = v.parse::<u32>().ok();
            match (*letter, n) {
                ('G', Some(85)) => slot = true,
                ('G', Some(0..=3)) => {
                    if !self.routing {
                        self.out.skipped_slots += 1;
                    }
                    self.routing = true;
                    slot = true;
                }
                ('G', Some(5)) | ('M', Some(16 | 17)) => self.routing = false,
                ('G', Some(91)) => return Err(self.err(line, "incremental mode (G91) unsupported")),
                ('M', Some(71)) => self.unit = Some(Unit::Mm),
                ('M', Some(72)) => self.unit = Some(Unit::Inch),
                _ => {}
            }
        }
        if let (Some(t), Some(c)) = (value('T'), value('C')) {
            return self.define_tool(t, c, line);
        }
        if let Some(t) = value('T') {
            let n: u32 = t
                .parse()
                .map_err(|_| self.err(line, format!("bad tool number T{t}")))?;
            if n == 0 {
                self.tool = None;
            } else if self.tools.contains_key(&n) {
                self.tool = Some(n);
            } else {
                return Err(self.err(line, format!("tool T{n} used before it is defined")));
            }
        }
        if slot || self.routing {
            if slot && !self.routing {
                self.out.skipped_slots += 1;
            }
            return Ok(());
        }
        if value('X').is_some() || value('Y').is_some() {
            self.drill(value('X'), value('Y'), line)?;
        }
        Ok(())
    }

    fn define_tool(&mut self, t: &str, c: &str, line: usize) -> Result<()> {
        let n: u32 = t
            .parse()
            .map_err(|_| self.err(line, format!("bad tool number T{t}")))?;
        let unit = self
            .unit
            .ok_or_else(|| self.err(line, "tool defined before METRIC/INCH"))?;
        let dia = unit.to_mm(
            c.parse::<f64>()
                .map_err(|_| self.err(line, format!("bad tool diameter {c:?}")))?,
        );
        if !(dia.is_finite() && dia > 0.0 && dia < 100.0) {
            return Err(self.err(line, format!("implausible tool diameter {dia} mm")));
        }
        self.tools.insert(n, dia);
        Ok(())
    }

    fn drill(&mut self, x: Option<&str>, y: Option<&str>, line: usize) -> Result<()> {
        let unit = self
            .unit
            .ok_or_else(|| self.err(line, "hole before METRIC/INCH header"))?;
        let tool = self
            .tool
            .ok_or_else(|| self.err(line, "hole before any tool is selected"))?;
        let diameter = self.tools.get(&tool).copied().unwrap_or_default();
        let fmt = self.format();
        let decode = |raw: Option<&str>, fallback: f64| -> Result<f64> {
            raw.map_or(Ok(fallback), |r| {
                fmt.parse(r)
                    .map(|v| unit.to_mm(v))
                    .filter(|v| v.abs() <= MAX_COORD_MM)
                    .ok_or_else(|| self.err(line, format!("bad coordinate {r:?}")))
            })
        };
        self.pos = Point::new(decode(x, self.pos.x)?, decode(y, self.pos.y)?);
        self.out.holes.push(Hole {
            pos: self.pos,
            diameter,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn explicit_decimal_metric_like_easyeda() {
        let d = parse(
            "M48\n;DRILL file {EasyEDA}\nFMAT,2\nMETRIC,TZ\nT1C0.800\nT2C2.200\n%\nG90\nG05\nM71\n\
             T1\nX112.46Y80.0\nX117.54\nT2\nX105Y55\nM30\n",
            "pth.DRL",
        )
        .unwrap();
        assert_eq!(d.holes.len(), 3);
        assert!(close(d.holes[0].diameter, 0.8) && close(d.holes[0].pos.x, 112.46));
        assert!(close(d.holes[1].pos.x, 117.54) && close(d.holes[1].pos.y, 80.0)); // Y is modal
        assert!(close(d.holes[2].diameter, 2.2));
    }

    #[test]
    fn metric_3_3_integer_coordinates() {
        let d = parse(
            "M48\nMETRIC,TZ\n;FILE_FORMAT=3:3\nT1C1.0\n%\nT1\nX12345Y-6789\nM30",
            "f",
        )
        .unwrap();
        assert!(close(d.holes[0].pos.x, 12.345) && close(d.holes[0].pos.y, -6.789));
    }

    #[test]
    fn metric_4_2_precision_from_file_format_comment() {
        let d = parse(
            "M48\nMETRIC,TZ\n;FILE_FORMAT=4:2\nT1C1.0\n%\nT1\nX123456Y7890\nM30",
            "f",
        )
        .unwrap();
        assert!(close(d.holes[0].pos.x, 1234.56) && close(d.holes[0].pos.y, 78.90));
    }

    #[test]
    fn inch_with_lz_pads_trailing_zeros() {
        // INCH 2:4, leading zeros kept (trailing omitted): "01" -> 010000 = 1.0 in, "005" -> 0.5 in
        let d = parse("M48\nINCH,LZ\nT1C0.0394\n%\nT1\nX01Y005\nM30", "f").unwrap();
        assert!(
            close(d.holes[0].pos.x, 25.4) && close(d.holes[0].pos.y, 12.7),
            "{d:?}"
        );
        assert!(close(d.holes[0].diameter, 0.0394 * 25.4));
    }

    #[test]
    fn inch_tz_default_precision() {
        let d = parse("M48\nINCH,TZ\nT1C0.04\n%\nT1\nX10000Y5000\nM30", "f").unwrap();
        assert!(close(d.holes[0].pos.x, 25.4) && close(d.holes[0].pos.y, 12.7));
    }

    #[test]
    fn digit_pattern_in_unit_line() {
        let d = parse("M48\nMETRIC,000.00\nT1C1\n%\nT1\nX12345\nM30", "f").unwrap();
        assert!(close(d.holes[0].pos.x, 123.45), "{d:?}");
    }

    #[test]
    fn slots_and_routes_are_skipped() {
        let d = parse(
            "M48\nMETRIC\nT1C1.0\n%\nT1\nX1.0Y1.0\nX2.0Y2.0G85X4.0Y2.0\nG00X6.0Y6.0\nM15\nG01X9.0Y6.0\n\
             M16\nG05\nX3.0Y3.0\nM30",
            "f",
        )
        .unwrap();
        assert_eq!(d.holes.len(), 2);
        assert_eq!(d.skipped_slots, 2);
    }

    #[test]
    fn missing_pieces_are_errors() {
        for (text, needle) in [
            ("M48\nT1C1.0\n%\nT1\nX1Y1", "before METRIC"),
            ("M48\nMETRIC\n%\nX1.0Y1.0", "before any tool"),
            ("M48\nMETRIC\nT1C1.0\n%\nT2\nX1.0", "T2 used before"),
            ("M48\nMETRIC\nT1C-1\n%", "implausible"),
            ("M48\nMETRIC\nT1C1.0\n%\nT1\nXabc", "bad coordinate"),
            ("M48\nMETRIC\nT1C1.0\n%\nT1\nX1.0\nG91\n", "incremental"),
            ("M48\nBOGUS,1\n", "unrecognised"),
            (
                "M48\nMETRIC\nT1C1.0\n%\nT1\nX999999999.0Y0",
                "bad coordinate",
            ),
        ] {
            let e = parse(text, "x.DRL").unwrap_err().to_string();
            assert!(e.contains(needle), "{text:?} -> {e}");
        }
    }

    #[test]
    fn empty_file_yields_no_holes() {
        assert!(parse("", "f").unwrap().holes.is_empty());
    }
}
