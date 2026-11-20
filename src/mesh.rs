//! Indexed triangle mesh with the few solid-modelling primitives the cassette needs:
//! planar caps (polygon with holes, triangulated by ear clipping) and walls/frusta between
//! rings. Vertices are welded, so a surface assembled from caps and walls that share ring
//! vertices is watertight by construction; [`Mesh::topology`] verifies that.

use std::collections::HashMap;

use crate::error::{Error, Result};

/// A 2-D point `[x, y]`.
pub type P2 = [f64; 2];
/// A 3-D point `[x, y, z]`.
pub type P3 = [f64; 3];

/// A planar region: an outer boundary and holes cut out of it.
#[derive(Debug, Clone, Default)]
pub struct Region {
    /// Outer boundary (any winding).
    pub outer: Vec<P2>,
    /// Holes (any winding), each strictly inside `outer` and disjoint from each other.
    pub holes: Vec<Vec<P2>>,
}

/// Welded vertices are keyed on a 1e-6 mm grid.
const WELD_SCALE: f64 = 1e6;

#[allow(clippy::cast_possible_truncation)] // coordinates are bounded to a few metres
fn quantize(v: f64) -> i64 {
    (v * WELD_SCALE).round() as i64
}

/// An indexed triangle soup that is expected to bound a solid (outward-facing normals).
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    verts: Vec<P3>,
    lookup: HashMap<[i64; 3], u32>,
    tris: Vec<[u32; 3]>,
}

/// Result of [`Mesh::topology`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Topology {
    /// Number of triangles.
    pub triangles: usize,
    /// Edges used by exactly one triangle (holes in the surface).
    pub open_edges: usize,
    /// Edges used by more than two triangles.
    pub non_manifold_edges: usize,
    /// Edges shared by two triangles that traverse it in the *same* direction (flipped face).
    pub inconsistent_edges: usize,
    /// Triangles whose three vertices are not distinct points.
    pub degenerate_triangles: usize,
}

impl Topology {
    /// Every edge is shared by exactly two consistently oriented triangles.
    pub fn is_watertight(&self) -> bool {
        self.triangles > 0
            && self.open_edges == 0
            && self.non_manifold_edges == 0
            && self.inconsistent_edges == 0
            && self.degenerate_triangles == 0
    }
}

impl Mesh {
    /// An empty mesh.
    pub fn new() -> Self {
        Self::default()
    }

    fn vertex(&mut self, p: P3) -> u32 {
        let key = [quantize(p[0]), quantize(p[1]), quantize(p[2])];
        if let Some(&i) = self.lookup.get(&key) {
            return i;
        }
        #[allow(clippy::cast_possible_truncation)] // < 2^32 vertices by construction
        let i = self.verts.len() as u32;
        self.verts.push(p);
        self.lookup.insert(key, i);
        i
    }

    /// Add one triangle (counter-clockwise seen from outside the solid).
    pub fn triangle(&mut self, a: P3, b: P3, c: P3) {
        let t = [self.vertex(a), self.vertex(b), self.vertex(c)];
        self.tris.push(t);
    }

    /// Number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.tris.len()
    }

    /// Number of distinct (welded) vertices.
    pub fn vertex_count(&self) -> usize {
        self.verts.len()
    }

    /// All triangles as coordinate triples.
    pub fn triangles(&self) -> impl Iterator<Item = [P3; 3]> + '_ {
        self.tris.iter().map(|t| t.map(|i| self.verts[i as usize]))
    }

    /// `(min, max)` corners, or `None` for an empty mesh.
    pub fn bounds(&self) -> Option<(P3, P3)> {
        let first = *self.verts.first()?;
        Some(
            self.verts
                .iter()
                .fold((first, first), |(mut lo, mut hi), v| {
                    for k in 0..3 {
                        lo[k] = lo[k].min(v[k]);
                        hi[k] = hi[k].max(v[k]);
                    }
                    (lo, hi)
                }),
        )
    }

    /// Enclosed volume (positive when normals point outward) via the divergence theorem.
    pub fn signed_volume(&self) -> f64 {
        self.triangles()
            .map(|[a, b, c]| {
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum()
    }

    /// Check edge sharing and orientation.
    pub fn topology(&self) -> Topology {
        // per undirected edge (lo, hi): (uses lo->hi, uses hi->lo)
        let mut edges: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
        let mut degenerate = 0;
        for t in &self.tris {
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                degenerate += 1;
                continue;
            }
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let entry = edges.entry((a.min(b), a.max(b))).or_default();
                if a < b {
                    entry.0 += 1;
                } else {
                    entry.1 += 1;
                }
            }
        }
        let mut topo = Topology {
            triangles: self.tris.len(),
            degenerate_triangles: degenerate,
            ..Topology::default()
        };
        for &(fwd, bwd) in edges.values() {
            match (fwd, bwd) {
                (1, 1) => {}
                (a, b) if a + b == 1 => topo.open_edges += 1,
                (a, b) if a + b == 2 => topo.inconsistent_edges += 1,
                _ => topo.non_manifold_edges += 1,
            }
        }
        topo
    }

    /// Fill `region` at height `z`; `up` selects a +Z or -Z facing normal.
    pub fn cap(&mut self, region: &Region, height: f64, up: bool) -> Result<()> {
        let mut flat: Vec<f64> = Vec::new();
        let mut points: Vec<P2> = Vec::new();
        let mut hole_starts = Vec::new();
        for (n, ring) in std::iter::once(&region.outer)
            .chain(&region.holes)
            .enumerate()
        {
            if ring.len() < 3 {
                return Err(Error::Geometry(
                    "polygon ring with fewer than 3 points".into(),
                ));
            }
            if n > 0 {
                hole_starts.push(points.len());
            }
            for p in ring {
                flat.extend_from_slice(p);
                points.push(*p);
            }
        }
        let indices = earcutr::earcut(&flat, &hole_starts, 2)
            .map_err(|e| Error::Geometry(format!("triangulation failed: {e}")))?;
        for t in indices.chunks_exact(3) {
            let corner = |i: usize| [points[t[i]][0], points[t[i]][1], height];
            let (first, second, third) = (corner(0), corner(1), corner(2));
            let area2 = (second[0] - first[0]) * (third[1] - first[1])
                - (second[1] - first[1]) * (third[0] - first[0]);
            if (area2 > 0.0) == up {
                self.triangle(first, second, third);
            } else {
                self.triangle(first, third, second);
            }
        }
        Ok(())
    }

    /// Vertical wall along a counter-clockwise `ring` between `z0` and `z1`.
    /// `outward` faces away from the ring's interior (use `false` for hole walls).
    pub fn wall(&mut self, ring: &[P2], z0: f64, z1: f64, outward: bool) {
        self.band(ring, z0, ring, z1, outward);
    }

    /// Sloped band from `lower` (at `z0`) to `upper` (at `z1`); rings must have equal length
    /// and be counter-clockwise. Faces outward.
    pub fn frustum(&mut self, lower: &[P2], z0: f64, upper: &[P2], z1: f64) {
        self.band(lower, z0, upper, z1, true);
    }

    fn band(&mut self, lower: &[P2], z0: f64, upper: &[P2], z1: f64, outward: bool) {
        let n = lower.len().min(upper.len());
        for i in 0..n {
            let j = (i + 1) % n;
            let a0 = [lower[i][0], lower[i][1], z0];
            let b0 = [lower[j][0], lower[j][1], z0];
            let b1 = [upper[j][0], upper[j][1], z1];
            let a1 = [upper[i][0], upper[i][1], z1];
            if outward {
                self.triangle(a0, b0, b1);
                self.triangle(a0, b1, a1);
            } else {
                self.triangle(a0, b1, b0);
                self.triangle(a0, a1, b1);
            }
        }
    }
}

/// Counter-clockwise axis-aligned rectangle.
pub fn rect(min: P2, max: P2) -> Vec<P2> {
    vec![
        [min[0], min[1]],
        [max[0], min[1]],
        [max[0], max[1]],
        [min[0], max[1]],
    ]
}

/// Counter-clockwise regular polygon with `n` vertices on a circle of `radius` (vertex at angle 0).
pub fn circle(center: P2, radius: f64, n: usize) -> Vec<P2> {
    (0..n)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)] // n <= 128
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box built from caps + walls.
    fn cuboid() -> Mesh {
        let mut m = Mesh::new();
        let r = rect([0.0, 0.0], [2.0, 3.0]);
        let region = Region {
            outer: r.clone(),
            holes: vec![],
        };
        m.cap(&region, 0.0, false).unwrap();
        m.cap(&region, 4.0, true).unwrap();
        m.wall(&r, 0.0, 4.0, true);
        m
    }

    #[test]
    fn cuboid_is_watertight_with_correct_volume() {
        let m = cuboid();
        let t = m.topology();
        assert!(t.is_watertight(), "{t:?}");
        assert_eq!(m.vertex_count(), 8);
        assert!((m.signed_volume() - 24.0).abs() < 1e-9);
        assert_eq!(m.bounds(), Some(([0.0, 0.0, 0.0], [2.0, 3.0, 4.0])));
    }

    #[test]
    fn plate_with_round_hole_is_watertight_and_loses_volume() {
        let mut m = Mesh::new();
        let outer = rect([0.0, 0.0], [10.0, 10.0]);
        let hole = circle([5.0, 5.0], 1.0, 24);
        let region = Region {
            outer: outer.clone(),
            holes: vec![hole.clone()],
        };
        m.cap(&region, 0.0, false).unwrap();
        m.cap(&region, 2.0, true).unwrap();
        m.wall(&outer, 0.0, 2.0, true);
        m.wall(&hole, 0.0, 2.0, false);
        assert!(m.topology().is_watertight(), "{:?}", m.topology());
        let polygon_area = 0.5 * 24.0 * (std::f64::consts::TAU / 24.0).sin();
        assert!((m.signed_volume() - (100.0 - polygon_area) * 2.0).abs() < 1e-6);
    }

    #[test]
    fn flipped_face_and_missing_face_are_detected() {
        let mut m = cuboid();
        m.tris[0].swap(1, 2); // flip one triangle
        let t = m.topology();
        assert!(!t.is_watertight());
        assert!(t.inconsistent_edges > 0);
        let mut m = cuboid();
        m.tris.pop();
        assert!(m.topology().open_edges > 0);
    }

    #[test]
    fn rings_with_too_few_points_are_errors() {
        let mut m = Mesh::new();
        let bad = Region {
            outer: vec![[0.0, 0.0], [1.0, 0.0]],
            holes: vec![],
        };
        assert!(m.cap(&bad, 0.0, true).is_err());
    }

    #[test]
    fn circle_and_rect_are_counter_clockwise() {
        let area = |ring: &[P2]| -> f64 {
            (0..ring.len())
                .map(|i| {
                    let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                    a[0] * b[1] - b[0] * a[1]
                })
                .sum::<f64>()
                / 2.0
        };
        assert!(area(&circle([1.0, 1.0], 2.0, 16)) > 0.0);
        assert!(area(&rect([0.0, 0.0], [1.0, 1.0])) > 0.0);
    }
}
