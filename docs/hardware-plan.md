# DIY 4-axis CNC hot-wire wing cutter: final build (≈ $650)

*Nov 2026. A single recommended build. Budget target $600 ±20%, hard cap $720. Background, sources and rejected alternatives are in `research/01–04`. Prices are typical AliExpress / Amazon / Mean Well distributor street prices; check them when you order.*

---

## 1. What you're building

- **Two towers 1.6 m apart** on a rigid aluminium-extrusion base. It cuts **wing panels 1.0–1.25 m long**, so a 4 m wing is 4 panels: 2 × ~1.0–1.25 m centre panels + 2 tips.
- **Travel:** X (chord) 1000 mm, Y (height) 300 mm on each tower. The root tower drives X/Y and the tip tower drives Z/A.
- **Controller:** FluidNC on the **Bart Dring 4x TMC2209 ESP32 board**. Bart Dring wrote FluidNC, so this is its reference board. It reads its setup from a text file, so there's nothing to compile.
- **Power:** two separate Mean Well supplies: **24 V for the electronics and motors**, and **48 V feeding a Riden RD6006 constant-current module for the wire**. The wire runs at a set current (e.g. 2.00 A), so its temperature stays the same whatever the wire length, cutting angle or mains voltage.
- **Software:** WiHoWi (wing CAM) → devSim (simulate) → GRBL HotWire (send and control). All three are free. Section 5 explains each one.

```
            root tower (X/Y)                          tip tower (Z/A)
   ┌───────────┐                                         ┌───────────┐
   │ Y: T8×2   │   MIG tip ●━━━━━━━━ 0.4 mm NiCr ━━━━━━━━● MIG tip   │
   │ MGN12 300 │   (fixed clamp)       ~1.7 m          (bearing → 25 N │
   │           │                                        const-force spring)
   └─────┬─────┘      [ foam block ≤1250 × 600 × 150 ]   └─────┬─────┘
   X: GT2 belt, V-wheels on 2040, 1000 mm            X: same
   ══════╧════════════ 2040 base rail 1900 mm ═════════════════╧══════
   ══════════════════════ 2040 base rail 1900 mm ═════════════════════
          (3 cross-members; MDF bed 1300 × 600 between the towers)
```

---

## 2. Bill of materials

| # | Part | Spec / example | Qty | ≈ USD |
|---|---|---|---|---|
| **Frame** |||||
| 1 | 2040 V-slot extrusion | Base 2 × 1900, X beams 2 × 1100, cross-members 3 × 600, Y uprights 2 × 500 mm (~9 m) | 1 lot | 100 |
| 2 | Corner brackets, gusset plates, T-nuts, M5 screws | 40-pack kits | 1 lot | 35 |
| **Motion** |||||
| 3 | V-slot gantry plate + 4 V-wheels (eccentric spacers) | One per X carriage | 2 | 30 |
| 4 | MGN12H linear rail 400 mm + block | Y axis on each tower | 2 | 35 |
| 5 | GT2 6 mm belt 3 m, 20T 5 mm-bore pulley ×2, idler ×2, belt tensioners | X drive | 1 lot | 20 |
| 6 | T8×2 leadscrew 400 mm + anti-backlash nut + coupler + pillow block | Y drive (holds position with the power off) | 2 | 25 |
| 7 | NEMA17 stepper 2.0 A, 0.59 Nm (StepperOnline 17HS19-2004S1) | | 4 | 50 |
| 8 | Microswitch endstops (+100 nF capacitor each) | Homing on all 4 axes | 4 | 6 |
| 9 | Y carriage / wire-arm plates | 3D-printed PETG or 5 mm aluminium plate | 1 lot | 15 |
| **Electronics and power** |||||
| 10 | **Bart Dring 4x CNC controller (ESP32 + 4× TMC2209)** | [Elecrow](https://www.elecrow.com/4x-cnc-controller-integrated-esp32-and-tmc2209.html) | 1 | 49 + ~15 ship |
| 11 | **Mean Well LRS-150-24** (24 V 6.5 A) | Board + motors | 1 | 22 |
| 12 | **Mean Well LRS-150-48** (48 V 3.3 A) | Feeds the wire module | 1 | 25 |
| 13 | **Riden RD6006** (0–60 V, 0–6 A, constant voltage/constant current, 1 mA steps) | Wire current regulator, with live V/A display | 1 | 65 |
| 14 | Opto-isolated MOSFET module (≥10 A, ≥60 V) | Wire on/off from the controller's spindle-enable output | 1 | 5 |
| 15 | 22 mm mushroom E-stop, **2 NC contact blocks** | Contact 1 breaks the wire current; contact 2 goes to FluidNC's reset/estop input | 1 | 12 |
| 16 | IEC inlet with fuse + switch, inline fuses (3 A wire, 5 A 24 V), enclosure box, DIN/terminal blocks | Mains side fully enclosed | 1 lot | 30 |
| 17 | 4-core shielded motor cable 10 m, 1 mm² silicone wire for wire leads | | 1 lot | 25 |
| **Wire and tension** |||||
| 18 | 0.4 mm NiCr80 (Ni80Cr20) wire | 30 m reel (spares) | 1 | 10 |
| 19 | Constant-force spring ~25 N (2.5 kg), 624ZZ V-groove bearings ×2, MIG contact tips 0.8 mm (M6) ×4 | Tensioner and current feed | 1 lot | 18 |
| **Workspace** |||||
| 20 | 18 mm melamine/MDF bed 1300 × 600, fence strips, block hold-down weights | | 1 lot | 30 |
| 21 | 100 mm inline fan + 3 m duct to outside | Foam fumes | 1 | 35 |
| | **Total** | | | **≈ $650** |

**If you're over budget** (shipping/VAT): print the gantry plates (−$15), use plain T8 nuts (−$8), and buy the endstops and cable locally. That gets you to about $620. Don't cut corners on the Riden, the Mean Wells or the E-stop.

**If the 4x board is out of stock:** buy a **BTT SKR Pico V1.0** (~$36, 4× TMC2209) and flash **grblHAL** (choose the "SKR Pico Hotwire" board). Everything else, including the whole software chain, stays the same. Known SKR Pico snags: check the 4th-axis (A) enable-pin inversion, and that Trinamic UART is turned on for all 4 axes.

---

## 3. Why these parts (reliability)

| Choice | Reason |
|---|---|
| FluidNC 4x board | Made by FluidNC's own author, the reference hardware for the firmware. Configured by a text `config.yaml` (no compiling, nothing to break between versions). All 4 drivers are TMC2209 with current set in software. Wi-Fi web UI and SD card, so you can run a job without the USB link. G93 inverse-time feed, which tapered panels need. Supported by GRBL HotWire and WiHoWi. |
| Separate 24 V and 48 V Mean Well supplies | Wire switching can't put noise or brown-outs on the controller supply. Mean Well LRS units are cheap, certified and long-lived. |
| Riden RD6006 constant current | Wire temperature depends on current, not voltage. Constant current keeps kerf repeatable over a whole session and across cut angles. The live V/A display lets you watch wire resistance, so a failing contact shows up. It has 2× headroom: 0.4 mm NiCr at 1.7 m needs ~2.0 A × ~30 V ≈ 60 W. |
| On/off wire switching (not PWM) | PWM into a constant-current regulator fights its control loop. Set the heat in amps on the Riden; the controller only switches it on (`M3`) and off (`M5`). |
| Leadscrew on Y, belt on X | Y holds position when powered off, so the wire doesn't sink into the core. A belt makes a 1 m X axis fast and cheap. |
| Constant-force spring on the tower plus a rigid base | Keeps tension constant over the ~28 mm the wire length changes during a tapered cut. The base absorbs the 25 N pull between the towers. |
| MIG contact tips | Copper current feed and wire guide in one. No hot spots at clips, so far fewer snapped wires. |

---

## 4. Electrical layout

```
 Mains ─ IEC inlet (fuse + switch) ─┬─ LRS-150-24 ─ 5 A fuse ─ 4x controller ─ 4× NEMA17
                                    │                       │  spindle-enable out
                                    │                       ▼
                                    └─ LRS-150-48 ─ RD6006 (CC, e.g. 2.00 A, V-limit 45 V)
                                                     │ + ─ 3 A fuse ─ E-stop NC#1 ─ MOSFET ─ MIG tip ━ wire ━ MIG tip ─ −
                                       E-stop NC#2 ─ controller reset/estop input
```
- Use an **opto-isolated** MOSFET module (or a DC SSR rated ≥60 V/5 A) so the wire circuit stays electrically separate from the controller's ground.
- Starting points: **0.4 mm NiCr at 1.7 m ≈ 15 Ω cold**. Start at **1.8 A** and tune between 1.6 and 2.4 A on test blocks.
- Upgrade path to a 2 m tower spacing: 0.3 mm spring-hard 304 at ~1.5 A, about 40 V. The 48 V/RD6006 chain already covers it.

---

## 5. Software toolchain

You need one **Windows 10/11 PC** (or Windows in a VM). The wing-specific tools are Windows-only. A fully open-source, cross-platform fallback is listed after the main table.

| Step | Tool | What it does | Runs on / OS | Open source? | Code / download |
|---|---|---|---|---|---|
| 1. Airfoils | **UIUC Airfoil Coordinates Database** / Airfoil Tools | ~1,600 airfoil `.dat` coordinate files (root and tip sections) | Web browser | Free data | https://m-selig.ae.illinois.edu/ads/coord_database.html · https://airfoiltools.com |
| 2. Wing design and analysis | **XFLR5** v6 | Analyses airfoils (XFoil) and the wing/plane (VLM/panel): pick airfoils, check stall, washout, CG and stability for the 4 m wing | Windows, macOS, Linux (Qt desktop app) | **Yes**, GPL-3.0 | https://sourceforge.net/projects/xflr5/ |
| 3. Master wing definition | **LibreOffice Calc** (or any spreadsheet) | One table of span stations (chord, airfoil, twist, sweep offset, dihedral). Gives each panel's root and tip values so neighbouring panels match exactly | Windows, macOS, Linux | **Yes**, MPL-2.0 | https://www.libreoffice.org · https://git.libreoffice.org/core |
| 4. CAM: G-code | **WiHoWi** v1.13 (JP Gleyzes) | Root/tip airfoil (`.dat`/DXF/STL) → synchronised 4-axis G-code. Sweep, dihedral, separate root/tip kerf, spar and aileron cutouts, entry/exit paths, left/right flip. Outputs **XY = root, ZA = tip, in G93** (both towers arrive together). Wire heat as `S0–1000`. | Windows only, portable `.exe`, needs the .NET runtime | **No**: free (freeware), closed source, alpha, one developer | Download zips: https://hackaday.io/project/205440-wing-hot-wire-cnc-foam-cutter-gcode-streamer · Forum: https://forum.makerforums.info/t/wihowi-wing-hot-wire-free-cnc-4-axis-foam-cutter-software/94755 |
| 5. Simulate before cutting | **devSim Cnc Foam** (devCad) | Plays the G-code on a model of both towers and the block. Catches wrong projection, collisions and leaving the block | Windows only | **No**: free, closed source | https://www.devcad.com/eng/devsimcncfoam.asp |
| 6. Send / run the machine | **GRBL HotWire** V2.51 (rcKeith) | Connects to the controller, homes, jogs both towers together or separately, previews the cut in 2D/4D, streams the G-code, pauses and stops, switches wire heat. Has a FluidNC preset | Windows only | **No**: free, closed source. Built from the MIT-licensed GRBL Panel (https://github.com/gerritv/Grbl-Panel) | https://rckeith.co.uk/grbl-hotwire-for-cnc-foam-cutters/ |
| 7. Firmware (on the board) | **FluidNC** v4.1.1 (Sep 2026) | Real-time motion control on the ESP32: 4 axes, G93, homing, spindle output for the wire. Configured by `config.yaml`. Has a built-in **web UI** (Wi-Fi, SD card upload/run, status) | Runs on the controller's ESP32. You install it from Chrome/Edge on any OS via the web installer | **Yes**, GPL-3.0 | https://github.com/bdring/FluidNC · installer: https://installer.fluidnc.com · docs: http://wiki.fluidnc.com · web UI based on https://github.com/luc-github/ESP3D-WEBUI |

**Open-source fallback** (use it if a closed tool disappears, or if you don't run Windows):

| Role | Tool | OS | Licence / code |
|---|---|---|---|
| CAM | **FreeCAD** 1.x + **FoamCut** workbench: loft the wing in 3D, generate 4-axis paths on the tower planes with kerf and G93. No wing wizard; you model sweep and washout yourself | Windows, macOS, Linux | LGPL-2.1 · https://github.com/FreeCAD/FreeCAD · https://github.com/Shkolik/Foamcut (install from the Addon Manager) |
| Sender | **Universal G-code Sender (UGS)**: streams 4-axis files fine; ignore its preview of the 4th axis | Windows, macOS, Linux (Java) | GPL-3.0 · https://github.com/winder/Universal-G-Code-Sender |
| Sender (no PC) | **FluidNC web UI**: upload the file to the SD card and run it from a browser | Any browser | GPL-3.0 (part of FluidNC) |

**Workflow for each panel:** spreadsheet row → root/tip airfoil, chord, twist, offsets → WiHoWi (set tower spacing 1600 mm and the block position, kerf root/tip from your calibration table) → `.gcode` → devSim check → GRBL HotWire: home, set the wire current on the Riden, cut.

Note: GRBL HotWire's feed override doesn't work with G93 files. That's fine; control heat and speed through the CAM settings, not live overrides.

---

## 6. Build and commissioning steps

1. **Order.** Electronics and motors first (2–4 weeks from China). Extrusion and wood can be bought locally.
2. **Bench-test the electronics before building the frame.** Flash FluidNC with the web installer. Load the 4x board's 4-axis config from https://github.com/bdring/fluidnc-config-files and edit it: axes `x y z a`, all **linear**; steps/mm X/Z = **80** (GT2 20T, 1/16 microstep) and Y/A = **1600** (T8×2); motor current about 1.2 A; spindle type on/off on the spindle-enable pin. Test: every motor turns the right way, `M3`/`M5` switches the MOSFET, and the E-stop kills both wire current and motion.
3. **Frame.** Bolt up the base, set the towers 1600 mm apart, square them (diagonals equal within 1 mm), then fit the X carriages, belts, Y rails and leadscrews.
4. **Wire.** Fit MIG tips on both arms: one side a fixed clamp, the other over the bearing to the constant-force spring (≥60 mm free stroke).
5. **Motion check.** Home all 4 axes. Command a 1000 mm X move; both towers must end within 0.2 mm of each other.
6. **Heat and kerf calibration.** Use EPS 20–30 kg/m³. Cut 50 mm test blocks at 3 feeds × 3 currents (e.g. 1.6/1.9/2.2 A) and record the kerf. Pick one current, then build a kerf-vs-feed table; the root/tip kerf settings in WiHoWi come from it. A good cut leaves fine hairs and no drag marks.
7. **First panels.** A straight rectangular panel, then a tapered and swept one. Check every file in devSim. Start cuts at the leading edge and slow down there. Keep the top and bottom offcuts ("beds") for sheeting.
8. **The 4 m wing.** Finish the master spreadsheet, generate 4 panels, cut them, cut the spar slots, then join and sheet.

---

## 7. Safety and gotchas
- Extraction to outdoors. Wire at the lowest current that cuts cleanly. **Never hot-wire PIR/PU foam.** Never leave the machine running unattended. Keep an extinguisher close.
- The mains side (IEC inlet, PSU terminals) must be fully enclosed. Fuse the wire circuit at 3 A.
- Wires can snap under 25 N: wear eye protection.
- Weight or pin the foam block (≈10 kg) and square it against the fence; a moving block causes twisted panels.
- Expect wire lag (a blunt trailing edge and a flattened leading edge). Fix it with slower feed, LE/TE dwell, and leaving the TE ~1 mm thick.

## 8. Known risks
- **WiHoWi** is closed-source alpha software by one person. Confirm on a cheap block that it handles the wire running past the block (tower projection) correctly. FreeCAD + FoamCut is the fallback.
- **Board stock** at Elecrow can be stale. The SKR Pico + grblHAL fallback is in §2.
- Prices are estimates, not quotes. The wire current/voltage figures are calculated; confirm them on test cuts.
