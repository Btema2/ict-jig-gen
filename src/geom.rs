//! Tiny 2-D geometry primitives shared by the parsers and the mesh builder. Units are mm.

/// A point in the plane.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
}

impl Point {
    /// Construct a point.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Euclidean distance to `other`.
    pub fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    #[must_use]
    /// Midpoint between `self` and `other`.
    pub fn midpoint(self, other: Self) -> Self {
        Self::new(
            f64::midpoint(self.x, other.x),
            f64::midpoint(self.y, other.y),
        )
    }
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    /// Lower-left corner.
    pub min: Point,
    /// Upper-right corner.
    pub max: Point,
}

impl BBox {
    /// A degenerate box containing just `p`.
    pub const fn around(p: Point) -> Self {
        Self { min: p, max: p }
    }

    /// Grow the box to include `p`.
    pub const fn include(&mut self, p: Point) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }

    /// Extent along X.
    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    /// Extent along Y.
    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }

    /// Whether `p` lies inside the box expanded by `tolerance` on every side.
    pub fn contains(&self, p: Point, tolerance: f64) -> bool {
        p.x >= self.min.x - tolerance
            && p.x <= self.max.x + tolerance
            && p.y >= self.min.y - tolerance
            && p.y <= self.max.y + tolerance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_grows_and_measures() {
        let mut b = BBox::around(Point::new(1.0, 2.0));
        b.include(Point::new(-1.0, 5.0));
        assert!((b.width() - 2.0).abs() < 1e-12);
        assert!((b.height() - 3.0).abs() < 1e-12);
        assert!(b.contains(Point::new(0.0, 3.0), 0.0));
        assert!(!b.contains(Point::new(2.0, 3.0), 0.5));
    }
}
