//! Fixed-point coordinate decoding shared by the Gerber and Excellon parsers.

/// Length unit declared by a fabrication file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Millimetres.
    Mm,
    /// Inches.
    Inch,
}

impl Unit {
    /// Convert a value in this unit to millimetres.
    pub fn to_mm(self, value: f64) -> f64 {
        match self {
            Self::Mm => value,
            Self::Inch => value * 25.4,
        }
    }
}

/// Which zeros a file leaves out of its fixed-point numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeroOmit {
    /// Leading zeros are dropped (Gerber `L`, Excellon `TZ`): `15` with 3 decimals is `0.015`.
    Leading,
    /// Trailing zeros are dropped (Gerber `T`, Excellon `LZ`): `15` with 3:3 digits is `150.0`.
    Trailing,
}

/// Fixed-point layout of coordinates: `int_digits` before the point, `dec_digits` after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordFormat {
    /// Digits before the decimal point.
    pub int_digits: u8,
    /// Digits after the decimal point.
    pub dec_digits: u8,
    /// Zero suppression mode.
    pub omit: ZeroOmit,
}

impl CoordFormat {
    /// Decode a raw coordinate token such as `-12345`, `+7`, or `12.5` (explicit decimal point).
    ///
    /// Returns `None` for anything that is not a finite plain number.
    pub fn parse(self, raw: &str) -> Option<f64> {
        let raw = raw.trim();
        let (negative, body) = match raw.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, raw.strip_prefix('+').unwrap_or(raw)),
        };
        if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
            return None;
        }
        let magnitude = if body.contains('.') {
            body.parse::<f64>().ok()?
        } else {
            let total = usize::from(self.int_digits) + usize::from(self.dec_digits);
            let scaled = match self.omit {
                ZeroOmit::Leading => body.to_owned(),
                ZeroOmit::Trailing => format!("{body:0<total$}"),
            };
            scaled.parse::<f64>().ok()? / 10f64.powi(i32::from(self.dec_digits))
        };
        magnitude
            .is_finite()
            .then_some(if negative { -magnitude } else { magnitude })
    }
}

/// Split `G01X100Y-200D01` into `[('G',"01"), ('X',"100"), ('Y',"-200"), ('D',"01")]`.
pub(crate) fn tokens(word: &str) -> Option<Vec<(char, &str)>> {
    let mut out = Vec::new();
    let mut chars = word.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if !c.is_ascii_alphabetic() {
            return None;
        }
        let value_start = start + c.len_utf8();
        let mut end = value_start;
        while let Some(&(idx, d)) = chars.peek() {
            if d.is_ascii_digit() || matches!(d, '+' | '-' | '.') {
                end = idx + d.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        out.push((c.to_ascii_uppercase(), &word[value_start..end]));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn fmt(i: u8, d: u8, omit: ZeroOmit) -> CoordFormat {
        CoordFormat {
            int_digits: i,
            dec_digits: d,
            omit,
        }
    }

    #[test]
    fn leading_zero_omission() {
        let f = fmt(3, 3, ZeroOmit::Leading);
        assert_eq!(f.parse("12345"), Some(12.345));
        assert_eq!(f.parse("-15"), Some(-0.015));
        assert_eq!(f.parse("0"), Some(0.0));
    }

    #[test]
    fn trailing_zero_omission_pads_right() {
        let f = fmt(3, 3, ZeroOmit::Trailing);
        assert_eq!(f.parse("15"), Some(150.0));
        assert_eq!(f.parse("12345"), Some(123.45));
        assert_eq!(f.parse("123456"), Some(123.456));
    }

    #[test]
    fn four_two_precision() {
        let f = fmt(4, 2, ZeroOmit::Leading);
        assert_eq!(f.parse("123456"), Some(1234.56));
    }

    #[test]
    fn explicit_decimal_point_wins() {
        let f = fmt(2, 4, ZeroOmit::Trailing);
        assert_eq!(f.parse("-3.5"), Some(-3.5));
        assert_eq!(f.parse("+.25"), Some(0.25));
    }

    #[test]
    fn rejects_garbage() {
        let f = fmt(3, 3, ZeroOmit::Leading);
        for bad in ["", "-", "1e5", "abc", "1.2.3", "inf", "--1"] {
            assert_eq!(f.parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn tokenizer_splits_letter_number_pairs() {
        assert_eq!(
            tokens("G01X100Y-2.5D01"),
            Some(vec![('G', "01"), ('X', "100"), ('Y', "-2.5"), ('D', "01")])
        );
        assert_eq!(tokens("T1C0.8"), Some(vec![('T', "1"), ('C', "0.8")]));
        assert_eq!(tokens("12"), None);
    }

    #[test]
    fn inch_converts() {
        assert!((Unit::Inch.to_mm(1.0) - 25.4).abs() < 1e-12);
    }
}
