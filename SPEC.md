# wingcut: spec and build brief

**wingcut** is an open-source, Linux-first Rust tool that turns a wing definition into synchronised 4-axis G-code for a CNC hot-wire foam cutter. It replaces the closed, Windows-only WiHoWi in our toolchain, and it is built for one specific machine (see `docs/hardware-plan.md`).

Clean-room: write everything from scratch. Use only public documentation and general hot-wire knowledge. Never decompile or copy WiHoWi, devWing or other closed tools.

---

## 1. The machine (defaults, all configurable)

| Item | Value |
|---|---|
| Layout | Two independent towers facing each other. The wire is stretched between them. |
| Tower spacing (wire contact to wire contact) | 1600 mm |
| Root tower axes | X (horizontal, chord direction) 0–1000 mm, Y (vertical) 0–300 mm |
| Tip tower axes | Z (horizontal) 0–1000 mm, A (vertical) 0–300 mm. All linear. Axis letters must be configurable (e.g. `X Y U V`). |
| Controller | FluidNC (ESP32): grbl-compatible G-code, supports **G93 inverse-time feed** |
| Wire heat | A constant-current supply set by hand. The controller only switches it with `M3 S<n>` / `M5`. The S value is configurable (default 1000). |
| Cutting speed | Typically 2–4 mm/s (120–240 mm/min). Rapids ≤ 1000 mm/min. |
| Max panel | 1250 mm span × 600 mm chord direction × 150 mm thick foam block |

## 2. Coordinate system

- `s` = spanwise distance from the **root tower wire contact** (0) to the **tip tower contact** (1600).
- `x` = horizontal, along the chord direction, positive away from the operator (or as configured). `y` = vertical, up.
- A foam block sits between the towers. Its root face is at `s_r` and its tip face at `s_t` (e.g. a 1250 mm panel centred: `s_r = 175`, `s_t = 1425`).
- **Tower projection.** The wire is a straight line through the root-face point `P_r` and tip-face point `P_t`:
  `P(s) = P_r + (P_t − P_r) · (s − s_r) / (s_t − s_r)`.
  The root tower command is `P(0)`, the tip tower command is `P(1600)`. Evaluate this for every synchronised point pair.
- Panels can be placed root-left or flipped (left/right wing). Mirroring is a config switch.

## 3. Wing definition (input)

TOML files. Suggested shape (refine as needed, but keep them human-editable):

```toml
# wing.toml
name = "glider-4m"
units = "mm"

[[station]]           # span stations, along the half-span from the centreline
span = 0
chord = 260
airfoil = "airfoils/ag35.dat"
le_offset = 0         # sweep: x offset of the leading edge, positive = aft
dihedral_offset = 0   # y offset (dihedral/polyhedral)
twist_deg = 0         # washout negative, rotated about twist_pivot
twist_pivot = 0.25    # fraction of chord

[[station]]
span = 1000
chord = 220
airfoil = "airfoils/ag35.dat"
le_offset = 15
twist_deg = -0.5

[[station]]
span = 2000
chord = 140
airfoil = "airfoils/ag36.dat"
le_offset = 45
twist_deg = -2.0

[[panel]]             # one foam cut = one panel
name = "center-L"
from = 0              # span positions; interpolate airfoil/chord/twist/offsets linearly between stations
to = 1000
block = { s_root = 300, length = 1000, x0 = 100, y0 = 0, height = 60 }  # placement on the machine
side = "left"         # left/right mirroring
```

```toml
# machine.toml
tower_spacing = 1600
axes = ["X", "Y", "Z", "A"]   # root-x, root-y, tip-x, tip-y
limits = { x = [0, 1000], y = [0, 300] }
feed_cut = 180                # mm/min at the faster block face
feed_rapid = 800
heat_on = "M3 S1000"
heat_off = "M5"
preheat_s = 2.0
kerf = [ { feed = 120, kerf = 1.1 }, { feed = 180, kerf = 0.8 }, { feed = 240, kerf = 0.6 } ]  # mm, from test cuts; interpolate
```

Airfoil interpolation between two different `.dat` files: resample both to the same parameterisation (see §4), then blend coordinates point by point.

## 4. Pipeline per panel

1. **Parse `.dat`.** Handle Selig format (TE → upper → LE → lower → TE) and Lednicer format (upper and lower lists). Normalise to chord 1 and LE at (0,0). Detect and handle a closed vs. open TE.
2. **Resample with sync points.** Split at the LE (min x) and TE. Resample upper and lower surfaces separately with **cosine spacing** in x, with the same point count for root and tip. A dense default (~0.2 mm max segment at the largest chord) keeps G93 timing smooth. Later stages add spar notches as extra sync points.
3. **Build the section at the root and tip faces.** Interpolate chord, twist, offsets and airfoil to the panel's `from`/`to` span. Apply twist about the pivot, scale by chord, then add `le_offset`, `dihedral_offset` and the block placement (`x0`, `y0`). Enforce a minimum TE thickness (default 0.8 mm) so the TE doesn't burn away.
4. **Kerf compensation.** Offset each face profile **outward** by kerf/2. Kerf differs per face because the tip face moves slower. Look up the kerf from each face's actual speed using the machine's kerf table. Use a robust polyline offset; handle the sharp TE (miter limit).
5. **Cut path.** Approach from outside the block at TE height (rapid, wire off). Wire on, preheat dwell (`G4`). Lead-in to the TE, then cut **upper surface TE → LE → lower surface → TE**. Lead-out back out the TE side, wire off. Optional dwell at the LE (configurable) to reduce lag artefacts. The path must stay a single connected line so the top and bottom offcuts ("beds") remain usable for vacuum bagging.
6. **Project** every point pair to the towers (§2). **Check every projected point against the axis limits.** Fail loudly with the offending point if any are out of range.
7. **Feed / G93.** For each segment, compute its time from the configured speed applied to the **longer of the two face segments** (the faster face). Emit `G93` and `G1 X.. Y.. Z.. A.. F<1/t_minutes>`. Rapids and lead-ins can use G94. Always return to `G94` at the end.
8. **Emit G-code.** Header comments (panel name, wing, generator version, machine params, estimated time), `G21 G90`, moves, footer with `M5` and a safe park. Use 3 decimals and no scientific notation.

## 5. Outputs

- `out/<panel>.gcode` for each panel.
- `out/<panel>.svg`: overlay of the root-face and tip-face profiles (with kerf) and the two tower paths, plus the block outline and axis limits. This is the first-line visual check.
- A console summary: path length, max/min face speeds, estimated cut time, and kerf used per face.

## 6. CLI (stage 1–2 scope)

```
wingcut gen --machine machine.toml --wing wing.toml [--panel NAME] [-o out/]
wingcut check  ...   # validate configs and limits; no output
wingcut info airfoils/ag35.dat   # parse + print stats
```

## 7. Stages

| Stage | Scope | Done when |
|---|---|---|
| **1. Core** | `.dat` parsing, resampling with sync points, one straight/tapered panel, projection, kerf, G93 G-code, SVG preview, unit tests | `wingcut gen` produces valid G-code for a rectangular and a tapered/swept panel; tests pass |
| **2. Multi-panel wing** | Station interpolation, multiple panels from one wing.toml, left/right mirroring, twist/sweep/dihedral, TE min thickness, limit checks | A 4 m wing (4 panels) generates in one command, with matching root/tip at every joint (test this) |
| 3. GUI | `egui`/`eframe` app: edit configs, live 2D preview of both faces and tower paths, DXF profile import, spar notches, aileron/hinge cuts | Usable without hand-editing TOML |
| 4. Machine control | Talk to FluidNC (USB serial and/or WebSocket): connect, home, jog towers together/separately, stream with pause/stop. Optional: set the Riden RD6006 wire current over Modbus per job | Can run a full cut without GRBL HotWire |

**Start with stages 1 and 2 only.** Stop and report before starting the GUI.

## 8. Engineering rules

- Rust stable, Cargo workspace or a single crate (keep it simple). Linux is the primary target; don't break macOS or Windows needlessly.
- Few dependencies. Reasonable picks: `serde` + `toml`, `clap`, `anyhow`/`thiserror`. Also approved: `clap_complete` (shell completions); for the GUI crate: `eframe`, `egui_plot`, `rfd`. For polygon offsetting, pick one well-maintained crate, or write a small, well-tested offset. Justify any other dependency in the PR or commit message.
- Unit tests for every geometric step. Required tests:
  - Projection with block faces at the towers is the identity.
  - A rectangular untapered panel gives identical root and tip tower paths.
  - G93 segment times are equal for both towers.
  - Adjacent panels share an identical joint section.
  - Limit violations are reported.
- A sample `examples/` folder: two public-domain airfoil `.dat` files (e.g. from the UIUC database, noting the source), a `machine.toml`, and a 4-panel `wing.toml` for a 4 m glider.
- `cargo fmt`, `cargo clippy -D warnings` and `cargo test` must pass before each push.
- Licence: **MIT** (decided by the owner, Nov 2026). See `LICENSE`.

## 9. Validation (after stage 2)

1. Generate the same panel in wingcut and in WiHoWi (on Windows), then compare both in devSim Cnc Foam (free, Windows; https://www.devcad.com/eng/devsimcncfoam.asp).
2. Cut cheap EPS test blocks on the real machine. Measure the kerf and LE/TE quality, and feed the results back into the kerf table and the defaults.

## 10. Background

- `docs/hardware-plan.md`: the machine being built (BOM, wiring, toolchain).
- `docs/research/`: research on electronics, mechanics, software/CAM and costs, with sources. `03-software-cam.md` describes what existing CAM tools do.
