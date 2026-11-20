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

## Output contract: `lista_elementow.json`

Consumed by the Python desktop app (ICT-SCAN PRO). Do not change shape without the user's approval.

```json
{ "elementy": [ { "nazwa": "R1", "piny": [0, 1], "x": 168, "y": 240, "typ": "element" },
                { "nazwa": "VCC-GND", "piny": [0, 15], "x": 256, "y": 452, "typ": "linia" } ] }
```

- Top-level JSON **object** with the single key `"elementy"` holding an array of entries.
- Each entry has exactly `nazwa`, `piny`, `x`, `y`, `typ` (Polish, lowercase keys; code/docs stay English).
- `nazwa`: non-empty string (designator such as `R1`, or a rail such as `VCC-GND`). UTF-8, not escaped.
- `piny`: exactly 2 distinct integers, each in `0..=15` (multiplexer channels).
- `x`, `y`: **integers** (pixels). Origin `(0, 0)` is the top-left of the top-down PCB image
  (`zdjecie_plytki.png`). Linear map from the board-outline bounding box (mm) to the image size,
  Y flipped (Gerber +Y is up, image +Y is down). Image size comes from `--image <png|jpg>` or
  `--image-size WxH`; default `512x512`.
- `typ`: `"element"` or `"linia"` (power rails: `VCC-GND`, `5V-GND`, `3V3-GND`). No other values.

## Input scope

Accepted files from an EasyEDA export, as a `.zip` or an unzipped directory (layers are
discovered by name/extension, so Standard and Pro naming both work):

| File | Meaning | Required |
|------|---------|----------|
| `*.GKO` / `*.GM1` / outline `.gbr` | Board outline (RS-274X) | yes |
| `*PTH*.DRL` / `.TXT` (not `NPTH`, not `*Via*`) | Plated holes = probe targets | yes |
| `*NPTH*.DRL` | Mounting holes -> alignment posts | no (falls back to a corner lip) |
| `PickAndPlace_*.csv` | Component centroids (designators) | no |

Out of scope: any other Gerber layer (copper, mask, silk, paste), other EDA vendors.
Treat input as untrusted: malformed or missing files are errors, never panics.

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

- Claude Code reads `CLAUDE.md` (symlink to this file) and `.claude/` (settings, hooks).
- Antigravity (`agy`) reads this `AGENTS.md`, `.agents/skills/` and `.agents/hooks.json`.
- Same guardrails in both: rustfmt after edits, cargo check+clippy gate before stopping,
  and a hard block on `rm -rf`, force-push and `git reset --hard`. Claude: `.claude/settings.json`;
  agy: `.agents/hooks.json` -> `.agents/hooks/*.sh` (camelCase JSON on stdin, `decision` out).
- agy's permissions cannot live in the repo; its global config may be more permissive than
  Claude's (`always-proceed`), so the hooks above are the only repo-local enforcement for agy.
- Project-scoped only: nothing here depends on or alters `~/.claude` or `~/.gemini`.
