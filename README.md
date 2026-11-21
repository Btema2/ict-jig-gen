# ict-jig-gen

Pure-Rust CLI that turns an **EasyEDA Gerber/Excellon export** into

1. a 3D-printable, two-layer **pogo-pin test cassette** (`cassette.stl`, or with `--split` two
   support-free halves `cassette_bottom.stl` + `cassette_top.stl`) for a 16-channel ICT fixture;
   `--compact` trims it to the board outline and stands it on the board's mounting holes,
   which saves plastic and print time (and, with `--split`, the screws),
2. `lista_elementow.json`, the pin map read by the ICT-SCAN PRO desktop app,
3. `wiring_guide.txt`, which numbered wire goes into which hole of the printed cassette.

No external geometry tools are needed at runtime (`f3d` is only a dev-time previewer).

## Install and run

```bash
cargo build --release          # binary: target/release/ict-jig-gen
cargo run --release -- --help
```

### Quick start

```bash
# 1. See which plated holes the board offers (ids H1.., component pads R1.1, ...)
ict-jig-gen holes board_gerber.zip --pnp PickAndPlace_board.csv

# 2. Generate everything into ./out (<= 16 plated holes are assigned to pins 0.. automatically)
ict-jig-gen generate board_gerber.zip \
    --pnp PickAndPlace_board.csv \
    --image zdjecie_plytki.png \
    -o out

# 3. Read the pin numbers in out/wiring_guide.txt, then add rails between two of them
ict-jig-gen generate board_gerber.zip --pnp PickAndPlace_board.csv \
    --image zdjecie_plytki.png --rail VCC-GND=4,5 -o out
```

Rails and elements may only use pins that have a probe, so look the numbers up in the wiring
guide first. On a board with more than 16 holes, assign pins explicitly (next section).

The input is the `.zip` EasyEDA gives you or the unzipped folder. Layers are found by name, so
Standard (`Gerber_BoardOutlineLayer.GKO`, `Drill_PTH_Through.DRL`) and Pro exports both work:

| Layer | Used for |
|---|---|
| `*.GKO` / `*.GM1` / outline `.gbr` | board outline and the pixel mapping (required) |
| `*PTH*.DRL` (not `NPTH`, not `*Via*`) | plated holes = probe targets (required) |
| `*NPTH*.DRL` | mounting holes -> alignment posts (optional) |
| `PickAndPlace_*.csv` | designators and component positions (optional; found in the package or via `--pnp`; tab/comma, UTF-8/UTF-16, `mm`/`mil` suffixes) |

Metric and imperial Excellon files in the usual precisions (3:3, 4:2, 2:4, explicit decimal
points, `TZ`/`LZ`) are understood. Malformed input is reported as an error naming the file (and line where known),
never a panic. Unusable Pick & Place rows (`R?` duplicates, blank cells) are skipped with a warning.

### Try it on the bundled fixture

```bash
ict-jig-gen generate tests/fixtures/demo_board_gerber.zip \
    --pnp tests/fixtures/PickAndPlace_demo_board.csv \
    --rail VCC-GND=4,5 --image-size 600x400 -o out   # warns: J1 (pins 4,5) is also derived
just preview out/cassette.stl out/iso.png        # needs f3d
just preview-top out/cassette.stl out/top.png
```

With `--split -o out`, preview `out/cassette_bottom.stl` and `out/cassette_top.stl` the same way.
Add `--compact` for the board-outline cassette standing on the 4 mounting holes (with `--split`:
two screwless halves).

`tests/fixtures/gen_fixture.py` regenerates the fixture: a 60 x 40 mm board, 4 mounting holes
and 13 plated pads (R1, R2, D1, D2, a 3-pin U1, a 2-pin power header J1).

## Choosing the probe points

* **16 plated holes or fewer, nothing configured:** all of them are used. Pins `0..` are given
  out in designator order (`D1.1, D1.2, J1.1, ... R1.1`), or in reading order without a
  Pick & Place file. Every component with exactly two probed pads becomes an `element`.
* **More than 16 holes:** the tool stops and asks for a selection. Use `ict-jig-gen holes` to
  list candidates, then pick:
  `--probe 0=R1.1 --probe 1=R1.2 --probe 2=H12 --probe 3=112.5,80`
  (`H<n>` hole id, `<designator>.<pad>`, or `X,Y` board mm). Giving any `--probe` turns
  automatic assignment off.
* **Elements and rails:** `--element NAME=A,B` (typ `element`) and `--rail NAME=A,B`
  (typ `linia`, e.g. `VCC-GND=14,15`). An explicit entry replaces an automatic one of the
  same name; `--no-auto-elements` switches automatic ones off.
* Everything above can live in a TOML/JSON file (`--config`); see
  [`examples/jig.toml`](examples/jig.toml). Flags override the file.

Component association uses the nearest Pick & Place centroid within 15 mm, and pad numbers
count left to right then top to bottom. Check `ict-jig-gen holes` if a part has an unusual
footprint and pin the pads down with `H<n>` instead.

## Mechanical parameters

| Flag | Default | Meaning |
|---|---|---|
| `--pogo` | `P75-LM2` | preset: `P75-LM2` (1.02 mm barrel, 16.54 mm long, 2 mm stroke) or `P100-E2` (1.36 mm, 33.35 mm, 6.35 mm) |
| `--barrel-diameter`, `--pogo-length`, `--pogo-stroke` | from preset | override a preset (the guide then says "modified"); an unknown `--pogo` name needs all three |
| `--include-vias` | off | also treat `*Via*.DRL` holes as probe candidates (pass it, or the same `--config`, to `holes` too so ids match) |
| `--clearance` | 0.15 | added to the barrel diameter for the printed bore (hole = 1.17 mm by default) |
| `--plate-thickness` | 3 | each guide plate |
| `--standoff-height` | 8 | gap between the plates |
| `--margin` | 8 (min 7); 0 with `--compact` | plate overhang around the outline; a `margin` in the config file counts as set, also for `--compact` |
| `--min-wall` | 0.8 | smallest printable wall between features |
| `--lip-height` | 1 | height of the registration lip |
| `--pcb-thickness` | 1.6 | sets alignment-post height |
| `--alignment` | `auto` | `auto`, `posts`, `lip`, `both`, `none` |
| `--split` / `--no-split` (`split = true`) | off | write `cassette_bottom.stl` + `cassette_top.stl` that print flat without supports, joined by 4 M3 screws (see [Split mode](#split-mode-print-without-supports)) |
| `--compact` / `--no-compact` (`compact = true`) | off | footprint = board outline, pillars on the mounting holes instead of the perimeter; needs >= 2 mounting holes (see [Compact mode](#compact-mode-less-plastic-no-screws)) |
| `--skip-mounting-hole X,Y` (`skip_mounting_holes = [[x, y]]`) | none | give the mounting hole at board X,Y (mm, within 0.5 mm, as `holes` lists it) no alignment post or compact pillar, e.g. a hole filled by a connector's locating peg; repeatable, any mode |
| `--image` / `--image-size` | `512x512` | pixel size for the JSON; `--image` reads a PNG/JPEG header, `--image-size WxH` wins |

## What the cassette looks like

```
 top guide plate    z2..z3   holes + alignment posts (or a registration lip)
 open cavity        z1..z2   square pillars at the corners/edges, open on all sides
 bottom guide plate z0..z1   flat base on Z = 0
```

Every pogo barrel passes through two concentric bores `standoff_height` apart, so it cannot
tilt. The board lies component-side up on the top plate; the plungers touch its underside
pads, so board X/Y map onto the cassette without mirroring. Cassette coordinates have their
origin at the lower-left corner of the footprint (board bounding box + margin).

* **Alignment:** `auto` uses posts through the board's non-plated mounting holes (>= 1.5 mm; post
  diameter = hole - clearance, with a tapered tip). With two or more it uses posts only; with
  exactly one it adds the lip too (one post cannot stop rotation); with none it uses just the
  lip, a 1 mm raised frame around the board outline (0.3 mm clearance). Plated holes >= 2.5 mm count as mounting
  holes when there is no NPTH file.
* **Checks:** the tool refuses two probe holes closer than `hole + min-wall` (1.97 mm for
  P75-LM2; 2.31 mm for P100-E2), holes that would clash with posts, pillars or the lip, and warns
  when the plate stack is too tall for the plunger to rise by its stroke above the top plate.
  `--compact` adds its own checks (see [Compact mode](#compact-mode-less-plastic-no-screws)).
* **Printing:** the top plate bridges the open cavity, so print with support enabled (the cavity
  is open on all sides, so supports can be removed and wires routed out sideways), or use
  `--split` to avoid supports altogether. The mesh is
  checked to be watertight before it is written (generation fails otherwise) and every
  coordinate is in millimetres.
* Holes are circumscribed polygons (32 segments), so the printed bore is never narrower than
  barrel + clearance. Calibrate `--clearance` for your printer and material.

## Split mode: print without supports

The one-piece cassette bridges the open cavity with its top plate, which needs supports.
`--split` (or `split = true` in the config file) writes two parts instead, and each lies flat on
`Z = 0` with nothing hanging in the air:

```bash
ict-jig-gen generate board_gerber.zip --pnp PickAndPlace_board.csv --split -o out
# out/cassette_bottom.stl  out/cassette_top.stl  (no cassette.stl)
```

| File | Contents | Print as exported |
|---|---|---|
| `cassette_bottom.stl` | bottom plate with the pogo bores, the perimeter pillars, a locating boss with a pilot hole on each of the 4 corner pillars | plate on the bed, pillars up |
| `cassette_top.stl` | top plate with the same pogo bores, 4 through-sockets over the corner pillars, alignment posts (or the registration lip) | underside on the bed, posts/lip up |

Both parts keep the cassette's XY frame, so the bores line up one-to-one. Do not rotate or
flip either file; the top plate is already in its print orientation.

**Hardware:** 4x M3 screws, pan, button or cap head (head up to 5.7 mm across). The length is
chosen for the pillar height and printed in `wiring_guide.txt`: **M3 x 10 mm** with the default
3 mm plates and 8 mm gap. No nuts: the screws cut their own thread in the 2.6 mm pilot holes.

**Joint:** each corner pillar carries a round boss (4.2 mm by default: the 2.6 mm pilot hole
plus `--min-wall` walls, never under 0.7 mm) that stands up through a socket in the top plate, which locates the two plates concentrically; the socket is boss + `--clearance`,
the same slip fit as the pogo bores, so calibrate `--clearance` once for both. The screw goes
through the top plate into the boss and clamps the plates together. With the default 3 mm plates the boss stops 1 mm below
the plate surface so the screw head, not the boss, bears on the plate. The lower-left boss is
always 0.3 mm larger than a plain socket (4.65 mm by default), so it cannot enter one and the top plate fits only
one way round. If a large `--clearance` leaves too little wall around the screw, generation stops with an error;
a `--min-wall` the 5 mm pillar cannot honour is capped and warned about.

**Assembly:**

1. Print both files as exported, without supports.
2. Fit the pogo pins and wires into the bottom part (`wiring_guide.txt` has the table).
3. Lower the top part onto the pillars, bores over the pogo barrels, bosses into the sockets.
4. Drive one M3 screw through each corner of the top plate. Snug is enough; do not overtighten PLA.

**Notes:** `--no-split` overrides `split = true` from a config file. With a registration lip
(boards without mounting holes) the margin is raised to about 8.4 mm if needed so the screw heads
clear the lip, and a warning says so. A mounting-hole post closer than a screw head to a corner
screw is refused with a message to increase `--margin`. Files from the other mode that an earlier
run left in the output directory are never deleted, but a warning names them. A warning is also
printed when the pillars are too short for a reliable M3 thread (increase `--standoff-height`).

Boards with two or more mounting holes can skip the screws: `--compact --split` joins the halves
on pillars at the mounting holes instead (next section).

## Compact mode: less plastic, no screws

The default cassette carries an 8 mm margin of plate around the board and square pillars in it.
`--compact` (or `compact = true`) drops both. The footprint becomes the board outline's bounding
box (margin 0 unless a margin is set with `--margin` or `margin =` in the config file), and the
plates stand on a **stepped pillar at each mounting hole** instead of on perimeter pillars and
screw bosses:

```
          ╻  post, tapered tip: goes into the board's mounting hole
   ━━━━━━━┃━━━━━━━━  top plate (z2..z3)
         ┌┸┐         shoulder: carries the top plate
   ━━━━━━┷━┷━━━━━━━  bottom plate (z0..z1)
```

* The **pin** is the mounting hole minus `--clearance` (the same diameter as an alignment post).
  It rises through the top plate and ends as the tapered post that locates the board.
* The **shoulder** between the plates is the pin socket plus a 1 mm ledge all round (4.2 mm for
  2.2 mm holes, 5.2 mm for M3 holes of 3.2 mm).
* Every mounting hole >= 1.5 mm gets a pillar. A hole that must stay free, for example one filled
  by a connector's locating peg, is left out with `--skip-mounting-hole X,Y` (board mm, as
  `ict-jig-gen holes` lists it; repeatable).

```bash
ict-jig-gen generate board_gerber.zip --pnp PickAndPlace_board.csv --compact -o out          # one piece
ict-jig-gen generate board_gerber.zip --pnp PickAndPlace_board.csv --compact --split -o out  # two parts, no screws
```

| Mode | Files | Printing |
|---|---|---|
| `--compact` | `cassette.stl`: pillars fused into both plates | the top plate still bridges the cavity: print with supports |
| `--compact --split` | `cassette_bottom.stl`: bottom plate + stepped pillars, pins up to the post tips; `cassette_top.stl`: flat top plate with the pogo bores and one socket (pin + `--clearance`) per pin | both flat on the bed, no supports, no overhangs, no hardware |

**Assembly (`--compact --split`):**

1. Print both files as exported.
2. Fit the pogo pins and wires into the bottom part (`wiring_guide.txt` has the table).
3. Slide the top plate down over the pins, bores over the pogo barrels, until it rests on the
   shoulders. It only goes on where its sockets meet the pins and its bores meet the barrels, so it
   cannot be fitted wrongly.
4. Lay the board on the plate, posts through its mounting holes. Nothing is screwed: pressing the
   board down in the fixture holds the stack together.

**Savings:** `wiring_guide.txt` compares the result with the cassette the same settings give
without `--compact` (with the default 8 mm margin when yours is below its 7 mm minimum). For the
bundled 60 x 40 mm demo board: 60 x 40 mm instead of 76 x 56 mm (-44 % footprint) and about
15 cm³ of plastic instead of 27 cm³ (-45 %, solid volume before infill). Print time drops roughly
in proportion to the plastic.

**Requirements and checks:**

* At least 2 mounting holes >= 1.5 mm, as `ict-jig-gen holes` lists them (non-plated, or plated
  >= 2.5 mm when the package has no NPTH file), not counting skipped ones. With fewer, generation
  stops and suggests the default cassette with `--alignment auto` or `lip`, which locates the board
  with a registration lip.
* `--alignment` must be `auto` or `posts`: a lip does not fit on the board outline.
* A pogo bore closer than `--min-wall` to a shoulder (the barrel passes it in the cavity), or two
  shoulders that would merge, stop generation with a message naming the pin and the mounting hole
  (board coordinates, as `ict-jig-gen holes` lists them).
* If a pogo bore or a shoulder would come within `--min-wall` of the cassette edge, the margin is
  widened just enough (rounded up to 0.1 mm) and a warning says so.
* With `--split` each pin stands free, so a pin thinner than 1.2 mm (a small hole with a large
  `--clearance`) stops generation; reduce `--clearance` or skip that hole.
* With `--split`, a warning notes when the pillars do not surround the middle of the top plate
  (every board with exactly two mounting holes, or holes along one side): the loose plate can then
  tip when the board is pressed, so press close to the posts.

`--no-compact` overrides `compact = true` from a config file. If that file also sets a margin
under 7 mm, pass `--margin` too.

## Output files

* `cassette.stl`: binary STL, Z up. With `--split` it is replaced by `cassette_bottom.stl` and
  `cassette_top.stl` (see above).
* `lista_elementow.json`: `{ "elementy": [ {"nazwa","piny":[a,b],"x","y","typ"} ] }` with integer
  pixels, origin top-left, Y flipped from the Gerber frame, mapped linearly from the outline
  bounding box to the image size. `typ` is `"element"` or `"linia"`. Contract details are in
  [`AGENTS.md`](AGENTS.md).
* `wiring_guide.txt` (also printed): wire number, X/Y in mm on the cassette, pad label.

## Limits

Only the outline and drill layers are read. Non-rectangular outlines use their bounding box. Slots
are ignored. Other EDA vendors' naming is not guessed beyond the patterns above.

## Development

```bash
just check            # fmt-check + clippy -D warnings + tests: the one verification entrypoint
just fmt
just preview <stl>    # isometric render with f3d (see the verify-stl skill)
```

## License

GPL-2.0-only, the same licence as the Linux kernel. See [LICENSE](LICENSE).
