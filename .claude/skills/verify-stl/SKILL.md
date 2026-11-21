---
name: verify-stl
description: Verify generated 3D cassette STL geometry. Invoke whenever you generate or modify cassette/STL/mesh geometry code, before claiming the geometry is correct. Runs geometry unit checks (bounding box, triangle count, watertight/manifold) and renders isometric and top-down PNGs with f3d for visual inspection.
---

# verify-stl

Use after any change that affects the generated cassette mesh. Do not report geometry as
correct until every step below has been done and its output actually read.

## 1. Geometry unit checks

Geometry code must have tests (e.g. `tests/geometry_*.rs`). If a check below has no test, add it.
Run them with `just test` (or `cargo nextest run geometry`). They must assert:

- **Bounding box** matches the expected cassette dimensions in mm (with tolerance), Z up.
- **Non-zero triangle count**; no NaN/inf vertices; no zero-area (degenerate) triangles.
- **Watertight / manifold**: every undirected edge is shared by exactly two triangles and the
  winding is consistent (each directed edge appears once). Report counts of offending edges.
- Pogo holes: diameter >= barrel diameter (default 1.02 mm) and positions match input drills.

## 2. Generate and render

Write the STL under `out/` (gitignored), then render both views headlessly:

```bash
mkdir -p out
cargo run -q -- <args> # produces out/cassette.stl (adapt to the CLI)
just preview     out/cassette.stl out/iso.png   # isometric, Z up
just preview-top out/cassette.stl out/top.png   # looking down -Z: X axis up, Y axis left
```

With `--split`, render both halves (`out/cassette_bottom.stl`, `out/cassette_top.stl`) and
check each is on Z = 0 and has no downward-facing triangle above the bed.

With `--compact` (one piece or `--split`), also check that the footprint is the board outline,
that there are no perimeter pillars or M3 bosses, and that a stepped pillar (shoulder, pin,
tapered tip) stands on every mounting hole not passed to `--skip-mounting-hole`. In split mode
the top half is a flat plate whose sockets sit exactly over the bottom half's pins.

If `just preview` fails, read the error. Do not add `--rendering-backend` (egl/osmesa/glx
are unavailable here; the default backend works headless).

## 3. Inspect the images

Open `out/iso.png` and `out/top.png` with an image-capable file read and look at them:

- The model is visible and fills the frame. A blank dark image means the render failed.
- Both layers present, holes are through-holes where expected, hole pattern is plausible
  (16 pogo positions), no missing walls, inverted faces, or stray geometry.
- Orientation matches the axes widget in the corner.

State what you saw in the images, concretely. If anything is off, fix it and repeat from step 1.

## 4. Finish

Run `just check`. Never commit `.stl` or `.png` files.
