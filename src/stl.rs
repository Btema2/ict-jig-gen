//! Binary STL reading and writing.

use std::io::{self, Write};

use crate::error::{Error, Result};
use crate::mesh::Mesh;

const HEADER_BYTES: usize = 80;
const TRIANGLE_BYTES: usize = 50;

/// Serialise `mesh` as binary STL (millimetres, right-handed, Z up).
pub fn write(mesh: &Mesh, mut out: impl Write) -> io::Result<()> {
    let mut header = [0u8; HEADER_BYTES];
    let label = b"ict-jig-gen pogo-pin cassette, mm";
    header[..label.len()].copy_from_slice(label);
    out.write_all(&header)?;
    let count = u32::try_from(mesh.triangle_count())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many triangles for STL"))?;
    out.write_all(&count.to_le_bytes())?;
    for [a, b, c] in mesh.triangles() {
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let mut n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let len = n.iter().map(|x| x * x).sum::<f64>().sqrt();
        if len > 0.0 {
            n = n.map(|x| x / len);
        }
        for value in n.into_iter().chain(a).chain(b).chain(c) {
            #[allow(clippy::cast_possible_truncation)] // STL stores f32
            out.write_all(&(value as f32).to_le_bytes())?;
        }
        out.write_all(&[0, 0])?; // attribute byte count
    }
    Ok(())
}

/// Parse a binary STL into a welded [`Mesh`].
pub fn read(bytes: &[u8]) -> Result<Mesh> {
    let bad = |m: &str| Error::Geometry(format!("invalid STL: {m}"));
    let count_bytes = bytes
        .get(HEADER_BYTES..HEADER_BYTES + 4)
        .ok_or_else(|| bad("too short"))?;
    let count = u32::from_le_bytes([
        count_bytes[0],
        count_bytes[1],
        count_bytes[2],
        count_bytes[3],
    ]);
    let body = &bytes[HEADER_BYTES + 4..];
    let expected = usize::try_from(count)
        .ok()
        .and_then(|c| c.checked_mul(TRIANGLE_BYTES));
    if expected != Some(body.len()) {
        return Err(bad(
            "size does not match the triangle count (ASCII STL is not supported)",
        ));
    }
    let float = |chunk: &[u8], i: usize| {
        let o = i * 4;
        f64::from(f32::from_le_bytes([
            chunk[o],
            chunk[o + 1],
            chunk[o + 2],
            chunk[o + 3],
        ]))
    };
    let mut mesh = Mesh::new();
    for tri in body.chunks_exact(TRIANGLE_BYTES) {
        let vertex = |k: usize| {
            [
                float(tri, 3 + 3 * k),
                float(tri, 4 + 3 * k),
                float(tri, 5 + 3 * k),
            ]
        };
        mesh.triangle(vertex(0), vertex(1), vertex(2));
    }
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{Region, rect};

    #[test]
    fn round_trip_preserves_triangles_and_watertightness() {
        let mut m = Mesh::new();
        let r = rect([0.0, 0.0], [5.0, 5.0]);
        let region = Region {
            outer: r.clone(),
            holes: vec![],
        };
        m.cap(&region, 0.0, false).unwrap();
        m.cap(&region, 1.5, true).unwrap();
        m.wall(&r, 0.0, 1.5, true);
        let mut buf = Vec::new();
        write(&m, &mut buf).unwrap();
        assert_eq!(buf.len(), 80 + 4 + 50 * m.triangle_count());
        let back = read(&buf).unwrap();
        assert_eq!(back.triangle_count(), m.triangle_count());
        assert!(back.topology().is_watertight());
        assert_eq!(back.bounds(), m.bounds());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(read(b"short").is_err());
        let mut buf = vec![0u8; 84];
        buf[80] = 5; // claims 5 triangles, has none
        assert!(read(&buf).is_err());
        assert!(read(b"solid ascii stl ...").is_err());
    }
}
