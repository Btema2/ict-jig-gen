# ict-jig-gen

Pure-Rust CLI (Linux/Fedora). Reads an EasyEDA Gerber + Excellon package and writes:

1. a 3D-printable two-layer pogo-pin test cassette (`.stl`) for a 16-pin ICT fixture
   (default pogo: P75-LM2, barrel diameter 1.02 mm);
2. `lista_elementow.json`, the pin map consumed by a companion desktop app.

No external geometry/CAD binaries at runtime. `f3d` is a dev-only previewer.

## Verification

- `just check` is the ONLY verification entrypoint (fmt-check, clippy `-D warnings`, tests).
  Run it and read its output before saying work is done. Zero warnings is the bar.
- Lints are strict: no `unwrap`/`expect`/`panic`/`unsafe` outside tests. Return errors.
- `just fmt` formats. A hook already runs rustfmt on every edited `.rs` file (Claude Code).
- Add dependencies only when needed; check current docs for the crate before using its API.

## Input scope

Accepted files from an EasyEDA export:

| File | Meaning | Required |
|------|---------|----------|
| `*.DRL` | Excellon drill (holes, tool diameters) | yes |
| `*.GKO` | Gerber board outline / keepout layer | yes |
| `*.csv` | Pick & Place (component centroids) | optional |

Out of scope: any other Gerber layer (copper, mask, silk, paste), other EDA vendors.
Treat input as untrusted: malformed or missing files are errors, never panics.

## Output contract: `lista_elementow.json`

Consumed by the companion app. Do not change shape without the user's approval.

- Each entry has exactly: `x`, `y` (pixel coordinates) and `typ`.
- `typ` is the string `"element"` or `"linia"` (Polish; lowercase; no other values).
- Keys are Polish/lowercase as above, even though code and docs are English.
- TODO(user): confirm and record here (top-level array vs. wrapper object, integer vs.
  float pixels, origin/axis orientation, pixel scale). Do not guess; ask.

## 3D output rules

- Two-layer cassette in one STL. Units are millimetres. Z is up (print orientation).
- Mesh must be watertight/manifold. Pogo holes must fit the barrel (1.02 mm default).
- Any change to cassette geometry requires the `verify-stl` skill before claiming success.

## `verify-stl` workflow

Skill: `.claude/skills/verify-stl/SKILL.md` (`.agents/skills` is a symlink to `.claude/skills`
so Antigravity `agy` finds it too).

1. Geometry unit tests: bounding box, non-zero triangles, watertight (each edge in 2 faces).
2. `just preview <file.stl> [out.png]` (isometric) and `just preview-top <file.stl> [out.png]`.
3. Open the PNGs and look at them. Passing tests alone is not enough.

`f3d` quirks (Fedora): default backend works headless; do NOT pass `--rendering-backend`
(egl/osmesa/glx fail: libs missing). Top-down view has X up, Y left. Use `--camera-direction`,
never `--camera-position` (a position outside the model silently renders a blank image).

## Git

- Conventional commits: `feat:`, `fix:`, `chore:`, `refactor:`, `test:`, `docs:`.
- Small, atomic commits; each one should pass `just check`.
- Never commit `.stl`/`.png` scratch files (gitignored); write them under `out/` or `/tmp`.
- Never force-push or `git reset --hard` (denied in `.claude/settings.json`).
- Do not edit `.claude/settings.local.json` into version control; it is per-user.

## Agent tooling

- Claude Code reads `CLAUDE.md` (symlink to this file) and `.claude/`.
- Antigravity (`agy`) reads this `AGENTS.md` and `.agents/skills/`.
- Project-scoped only: nothing here depends on or alters `~/.claude` or `~/.gemini`.
