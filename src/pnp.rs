//! Pick & Place (component centroid) CSV reader.
//!
//! `EasyEDA` writes tab- or comma-delimited text in UTF-8 or UTF-16 whose `Mid X` / `Mid Y`
//! values carry unit suffixes (`12.7mm`, `500mil`). Only the designator and centroid columns
//! are used.

use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::geom::Point;

/// One placed component.
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    /// Reference designator (`R1`, `U1`, ...).
    pub designator: String,
    /// Centroid in board coordinates, millimetres.
    pub pos: Point,
}

/// Parse a Pick & Place file from raw bytes (UTF-8, UTF-8 with BOM, or UTF-16 LE/BE).
pub fn parse(bytes: &[u8], file: &str) -> Result<Vec<Component>> {
    let text = decode(bytes, file)?;
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty());
    let (header_idx, header_line) = lines
        .by_ref()
        .find(|(_, l)| l.to_ascii_lowercase().contains("designator"))
        .ok_or_else(|| Error::parse(file, "no header row containing 'Designator'"))?;
    let delim = detect_delimiter(header_line);
    let header: Vec<String> = split_fields(header_line, delim)
        .iter()
        .map(|h| normalize(h))
        .collect();
    let find = |names: &[&str]| header.iter().position(|h| names.contains(&h.as_str()));
    let col_ref = find(&["designator", "reference", "refdes", "partref"])
        .ok_or_else(|| Error::parse_at(file, header_idx + 1, "missing Designator column"))?;
    let col_x = find(&["midx", "centerx", "posx", "x"])
        .ok_or_else(|| Error::parse_at(file, header_idx + 1, "missing 'Mid X' column"))?;
    let col_y = find(&["midy", "centery", "posy", "y"])
        .ok_or_else(|| Error::parse_at(file, header_idx + 1, "missing 'Mid Y' column"))?;

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (idx, line) in lines {
        let line_no = idx + 1;
        let fields = split_fields(line, delim);
        let get = |col: usize, what: &str| {
            fields
                .get(col)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| Error::parse_at(file, line_no, format!("row has no {what} value")))
        };
        let designator = get(col_ref, "Designator")?.to_owned();
        let coord = |col: usize, what: &str| -> Result<f64> {
            let raw = get(col, what)?;
            parse_length(raw, delim)
                .ok_or_else(|| Error::parse_at(file, line_no, format!("bad {what} value {raw:?}")))
        };
        let pos = Point::new(coord(col_x, "Mid X")?, coord(col_y, "Mid Y")?);
        if !seen.insert(designator.clone()) {
            return Err(Error::parse_at(
                file,
                line_no,
                format!("duplicate designator {designator}"),
            ));
        }
        out.push(Component { designator, pos });
    }
    Ok(out)
}

fn decode(bytes: &[u8], file: &str) -> Result<String> {
    let utf16 = |data: &[u8], big_endian: bool| -> Result<String> {
        if data.len() % 2 != 0 {
            return Err(Error::parse(file, "UTF-16 file has an odd number of bytes"));
        }
        let units: Vec<u16> = data
            .chunks_exact(2)
            .map(|c| {
                let pair = [c[0], c[1]];
                if big_endian {
                    u16::from_be_bytes(pair)
                } else {
                    u16::from_le_bytes(pair)
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| Error::parse(file, "invalid UTF-16 text"))
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, false),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => Ok(String::from_utf8_lossy(rest).into_owned()),
        // UTF-16 without BOM: ASCII text has a NUL in every other byte
        [a, 0, b, 0, ..] if *a != 0 && *b != 0 => utf16(bytes, false),
        [0, a, 0, b, ..] if *a != 0 && *b != 0 => utf16(bytes, true),
        _ => Ok(String::from_utf8_lossy(bytes).into_owned()),
    }
}

fn detect_delimiter(header: &str) -> char {
    [('\t', 0usize), (',', 1), (';', 2)]
        .iter()
        .map(|&(c, rank)| (header.matches(c).count(), 3 - rank, c))
        .max()
        .map_or(',', |(_, _, c)| c)
}

/// `"Mid X"` -> `midx`
fn normalize(header: &str) -> String {
    header
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Split one line on `delim`, honouring double-quoted fields.
fn split_fields(line: &str, delim: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == delim && !quoted => fields.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

/// `12.7mm`, `500mil`, `0.5in`, bare `3.2` (millimetres) -> millimetres.
fn parse_length(raw: &str, delim: char) -> Option<f64> {
    let lower = raw.trim().to_ascii_lowercase();
    let (number, scale) = [
        ("mils", 0.0254),
        ("mil", 0.0254),
        ("mm", 1.0),
        ("inch", 25.4),
        ("in", 25.4),
    ]
    .iter()
    .find_map(|&(suffix, scale)| lower.strip_suffix(suffix).map(|n| (n.trim(), scale)))
    .unwrap_or((lower.as_str(), 1.0));
    let number = if delim == ',' {
        number.to_owned()
    } else {
        number.replace(',', ".")
    };
    let value = number.parse::<f64>().ok()? * scale;
    value.is_finite().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    const TSV: &str = "Designator\tFootprint\tMid X\tMid Y\tLayer\tRotation\n\
                       R1\tAXIAL\t115mm\t80.5mm\tTop\t0\nD1\tLED\t135mm\t80mm\tTop\t90\n";

    #[test]
    fn tab_delimited_with_mm_suffix() {
        let c = parse(TSV.as_bytes(), "pnp.csv").unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].designator, "R1");
        assert!(close(c[0].pos.x, 115.0) && close(c[0].pos.y, 80.5));
    }

    #[test]
    fn comma_delimited_with_mil_suffix_and_quotes() {
        let csv = "\"Designator\",\"Footprint\",\"Mid X\",\"Mid Y\"\n\
                   \"U1\",\"SOIC,8\",\"1000mil\",\"500.5mil\"\n";
        let c = parse(csv.as_bytes(), "p.csv").unwrap();
        assert!(close(c[0].pos.x, 25.4) && close(c[0].pos.y, 500.5 * 0.0254));
    }

    #[test]
    fn utf16_le_with_bom_and_utf8_bom_and_bare_numbers() {
        let mut le = vec![0xFF, 0xFE];
        le.extend(TSV.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(parse(&le, "f").unwrap().len(), 2);
        let mut be = vec![0xFE, 0xFF];
        be.extend(TSV.encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(parse(&be, "f").unwrap().len(), 2);
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend(TSV.bytes());
        assert_eq!(parse(&bom, "f").unwrap().len(), 2);
        let bare = parse(b"Designator,Mid X,Mid Y\nR9,3.5,4.5\n", "f").unwrap();
        assert!(close(bare[0].pos.x, 3.5));
    }

    #[test]
    fn utf16_without_bom_is_detected() {
        let le: Vec<u8> = TSV.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(parse(&le, "f").unwrap().len(), 2);
    }

    #[test]
    fn column_order_is_free_and_extra_columns_ignored() {
        let c = parse(
            b"Layer,Mid Y,Ref X,Mid X,Designator\nTop,2mm,9mm,1mm,C1\n",
            "f",
        )
        .unwrap();
        assert!(close(c[0].pos.x, 1.0) && close(c[0].pos.y, 2.0));
    }

    #[test]
    fn blank_lines_and_preamble_are_tolerated() {
        let c = parse(b"\n\nDesignator,Mid X,Mid Y\n\nR1,1,2\n\n", "f").unwrap();
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn malformed_input_is_an_error() {
        for (text, needle) in [
            ("", "no header"),
            ("Foo,Bar\n1,2\n", "no header"),
            ("Designator,Mid X\nR1,1\n", "Mid Y"),
            ("Designator,Mid X,Mid Y\nR1,1\n", "no Mid Y"),
            ("Designator,Mid X,Mid Y\nR1,abc,2\n", "bad Mid X"),
            ("Designator,Mid X,Mid Y\nR1,1,2\nR1,3,4\n", "duplicate"),
            ("Designator,Mid X,Mid Y\nR1,1e999mm,2\n", "bad Mid X"),
        ] {
            let e = parse(text.as_bytes(), "p.csv").unwrap_err().to_string();
            assert!(e.contains(needle), "{text:?} -> {e}");
        }
        assert!(parse(&[0xFF, 0xFE, 0x41], "f").is_err());
    }
}
