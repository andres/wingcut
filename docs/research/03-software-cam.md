# 03 — CAM & sender software for a 4-axis hot-wire wing cutter (as of Nov 2026)

**Bottom line:** For CAM, use **WiHoWi** (free, Windows, under active development in 2026). Use the **FreeCAD FoamCut workbench** (LGPL, active) as the open-source backup and for unusual shapes. Run **FluidNC** firmware and send jobs with **hoWiGS** or **rcKeith GRBL HotWire**. Check every file in **devSim Cnc Foam** (free) before cutting. If your time is worth more than about €150, buy **devWing Foam 2**. It is still the most complete wing tool and the reference the others are measured against. Don't start with Jedicut, GMFC or the Python scripts on GitHub. They are stale or too thin for a 4 m multi-panel wing.

EUR→USD ≈ 1.15. Prices are approximate; "unverified" means I found no vendor price.

---

## 1. What the CAM must do (the checklist)

| Need | Why it matters | How to judge a tool |
|---|---|---|
| Different root and tip airfoils, synchronised | A tapered panel is a ruled surface. Root point *i* must meet tip point *i* | Resamples both airfoils to the same point count by % chord, and keeps LE/TE (and spars) as **sync points** |
| Projection to the towers | The wire is longer than the block. XY/UV are the tower planes, not the block faces | Takes block position and tower spacing. Extends the ruled line: `P_tower = P_root + (P_tip−P_root)·(z_tower−z_root)/(z_tip−z_root)` |
| Equal time per segment | Both ends must reach each point together. The tip side moves slower | Puts all 4 axes in one `G1` line. Best case it uses **G93 inverse-time** feed, so F means time per segment, not the 4-D vector speed that Grbl/FluidNC otherwise uses |
| Kerf that varies with speed | The slow (tip) end melts more | At minimum, separate root and tip kerf. Better: kerf as a function of speed, calibrated with test cuts |
| Sweep, washout, dihedral | Planform | Numeric inputs, not hand-edited DXFs |
| Spars, LE/TE, entry/exit | Spar slots, hinge cut, clean TE | Spar notches cut inside the profile path. Entry at TE height so the **beds** (the leftover top and bottom foam) survive for vacuum bagging |
| Sheeting offset | Skins of balsa, glass or obechi | Shrinks the airfoil inward by the skin thickness |
| Multi-panel | 4 m span means 4–6 panels of 0.7–1 m | Panel *n*'s tip = panel *n+1*'s root, so airfoil and twist blend continuously |

---

## 2. Wing CAM options

| Tool | Licence / price | Platform | Status | What it does well | Gaps / cons |
|---|---|---|---|---|---|
| **WiHoWi + hoWiGS** (JP Gleyzes "freedom2000") | Free, closed source | Windows (.NET) | Launched 3 Mar 2026. Updated 30 Mar 2026; Clipper2 kerf since v1.07 | Root/tip from DXF, DAT or STL with synchro points. Sweep and dihedral inputs. **Separate left/right kerf**. Spar and other cutouts, entry/exit chamfers, hollow wings, left/right flip. **G93 or G94**. Temperature `S` command. Top/bottom export modes. hoWiGS sender has live temperature and feed tuning. Targets FluidNC; also tested on Grbl | One developer, alpha stage. Washout is read from the DXF and can't be edited. Projection/tower geometry isn't documented (check it in devSim). Use polylines, not splines. [forum](https://forum.makerforums.info/t/wihowi-wing-hot-wire-free-cnc-4-axis-foam-cutter-software/94755), [hackaday](https://hackaday.io/project/205440-wing-hot-wire-cnc-foam-cutter-gcode-streamer/details) |
| **FreeCAD FoamCut WB** (Shkolik) | LGPL-2.1, free | Win/Linux/Mac | Last commit 18 Aug 2026, 24★; installs from the Addon Manager | Machine model (wire length, axis travel). Paths built **on the tower planes** from opposite edges/faces, true 4-axis taper. Projection mode (2-axis style). Kerf per object. **G93 is the default**. Entry/exit paths, route ordering. Also does 5-axis (rotary) | You need a lofted 3D wing in FreeCAD first (import airfoils, loft, cut panels). README says "many limitations". No wing wizard (no sweep, washout or spar inputs; you model those). [repo](https://github.com/Shkolik/Foamcut), [FreeCAD blog Jun 2026](https://blog.freecad.org/2026/06/19/exploring-the-foamcut-workbench/) |
| **devWing Foam 2** (devCad / Stefano Duranti, also makes Profili) | ≈ €125–175 lifetime (~$145–200) or €70/yr. Demo can't export G-code | Windows | Maintained, commercial | The reference. Kerf, washout, dihedral, spars, ailerons, lightening holes, rotated (swept) planforms, **G93**, sheeting (lofted and developed flat), left+right panels in one block, 3D simulation, Profili import | Paid, Windows only, licence tiers ("drone/military" tier costs more). [devcad](https://www.devcad.com/eng/devwingfoam_new.asp), [prices per rcKeith](https://rckeith.co.uk/foam-wing-free-cnc-software/) |
| **Gemini Aero Foam** | ≈ €40 lifetime (~$46) per rcKeith. Demo saves only 30 lines | Windows + Linux | v1.0.1.0, 22 Nov 2025 | DAT/DXF, spar and LE/TE cuts, kerf, **G93**, simulation. Pairs with its own Gemini Aero Cutter sender (grbl 1.1/grblHAL) | Small vendor. Price not on the vendor page. [site](https://geminiaerotools.com/geminiaerofoam.html) |
| **Profili 2 Pro** | Paid. **Pro edition needed for G-code**. Price unverified (devCad shop) | Windows | Mature | Huge airfoil database, analysis, foam panel cut files | Use it for airfoils and send panels on to devWing. [BYU guide](https://flow.byu.edu/me415/resources/foam-cutter) |
| **Jedicut** | Free; open-source plugins (GPL-3) | Windows | v2.4.2 (~2020). Plugins repo last touched 2019 | Profile wizard. GCode output plugin since 2.3.4 (2017). Heat/speed management. Kerf via the "skin" setting. DXF/DAT plugins | Stale. Outputs incremental G91. DXF imports can fail. No dihedral. Designed for parallel-port boards and only later given G-code. [jedicut.com](https://www.jedicut.com/en/download/), [GCode post](https://www.jedicut.com/en/blog/2017/01/) |
| **GMFC** (French) | Historically shareware ~$25 with a free variant | Windows | Legacy; gm.cnc.free.fr didn't load from here | Good heat calibration at two speeds. Native `.cut` format; devSim converts it to G-code | Dead end for a new build. [cncarena](https://en.cncarena.com/forum/thread/163361-software-question/) |
| **FoamWorks 4** | Paid, proprietary | Windows | v1.5.2.2 is only on mirrors; looks dormant | Old all-in-one | Skip. [alternativeto](https://alternativeto.net/software/foamworks/about) |
| **WingHelper** | ≈ €70; 1-week trial | Win/Mac/Linux | Active | Mainly balsa wing design and DXF export | G-code needs your own post-processor. [ThomasHeb notes](https://github.com/ThomasHeb/4AxisFoamCutter) |
| **Wing Designer 4 Hotwire** (druckgott) | Free web app | Browser | Dec 2025 | Inner/outer `.dat`, sweep, ailerons, holes, machine/foam parameters | Small project, few users. [repo](https://github.com/druckgott/wingdesigner_4_hotwire) |
| **rcKeith "Wing G-code"** (= swarfer/winggcode) | GPL-3, free | Python 3.9 (any OS) | Last push Jan 2021 | Selig DAT database, washout, TE thickness. XYUV/XYUZ/XZ-YZ output. "Write to AXIS" for LinuxCNC | **No kerf** (rcKeith adds ~2 mm of chord as a workaround), no spars. [repo](https://github.com/swarfer/winggcode) |
| **PHOENIX** (gregpinaud) | No licence | Python | Aug 2024 | **Multi-panel** wings for Grbl-Mega-5X | Barely documented. [repo](https://github.com/gregpinaud/PHOENIX) |
| **Hot_wire_QT** (mstrens) | No licence | Python/PyQt5 (Win exe) | Jul 2022, 5 commits | Root/tip .dat, sync points, rectangular/round spar and aileron cuts, built-in 4-axis GRBL control | No documented kerf or sweep. Stale. [repo](https://github.com/mstrens/Hot_wire_QT) |
| Aerofoils/Cutter, hotwing-cli, gregkrsak/gcode, NiCr | GPL | Mixed | Old or abandoned | Code to borrow ([Aerofoils](https://github.com/rbp28668/Aerofoils) does taper, washout and spar slots) | No kerf or projection |
| 2026 GitHub long tail: OpenFoamCut (Java), hotwire-studio (browser), FoamSync, HotWire4X, SafeFoam, ai-foam-cut, vheinitz/foamcut | Mixed | Mixed | 0–1★, all created 2026 | Some list G93, taper and spars | Unproven. Several look AI-generated. Watch them, don't depend on them. [GitHub search](https://github.com/search?q=hot+wire+foam&type=repositories&s=updated) |

**Fusion 360:** The free personal licence has no 4- or 5-axis CAM, and there is no maintained hot-wire post processor ([Autodesk idea thread](https://forums.autodesk.com/t5/fusion-360-ideastation-archived/4-axis-hot-wire-foam-cutter/idi-p/7244480)). Workable route: model the lofted wing in Fusion. Export root and tip section sketches as **DXF polylines** at each panel's end planes, then feed them to WiHoWi. Or export a STEP file to FreeCAD FoamCut. Don't write a Fusion post processor yourself.

---

## 3. Senders / control

| Sender | Status (latest) | Hot-wire fit |
|---|---|---|
| **hoWiGS** (WiHoWi companion) | 2026, free, Windows | Made for this job. Kill switch, pause, live temperature and feed override, 3D preview, guillotine and dihedral moves. FluidNC first |
| **rcKeith GRBL HotWire** | V2.51, Dec 2025; free download; Windows. Grew out of GRBL Panel | Made for this job. Wire current control in 1%/10% steps, feed override for G94 files, presets for Mega2560 (Mega-5X), grblHAL, FluidNC and SKR Pico. Not open source as far as I can tell. [page](https://rckeith.co.uk/grbl-hotwire-for-cnc-foam-cutters/) |
| **FluidNC WebUI** | FluidNC v4.1.1 (25 Sep 2026), very active | Runs in a browser over Wi-Fi, no PC driver. Upload to the SD card and run, so a slow USB link can't starve a long cut. Generic preview; A shows as a rotary axis. [repo](https://github.com/bdring/FluidNC) |
| **LinuxCNC AXIS** | Active | The only mainstream GUI with a real **XYUV foam view** (`[DISPLAY] FOAM=1`, sample `sim/axis/axis_foam`). Needs a PC with a parallel port or a Mesa card, and is hard to learn. [forum](https://forum.linuxcnc.org/16-stepconf-wizard/31021-foam-cutter-xyza-vs-xyuv) |
| **UGS** | v2.1.28 (3 Nov 2026) | Streams 4-axis Grbl/FluidNC files fine. Preview treats A as rotary, so ignore it. No heat control UI beyond macros |
| **CNCjs** | v1.11.5 (Sep 2026) | Same as UGS. Nice on a Raspberry Pi; macros can drive `M3 S` heat |
| **bCNC** | PyPI release Mar 2025 | Works but adds nothing for hot wire. Skip |
| **devSim Cnc Foam** | Free (devCad) | **Simulator only.** Shows both tower planes and the block. Imports most generators' G-code plus GMFC `.cut`. Use it as the check before any cut |

Firmware note: on Grbl-Mega-5X ([fra589](https://github.com/fra589/grbl-Mega-5X), May 2026), grblHAL and FluidNC, set the 4th axis up as **linear**. With G94, F is applied to the combined XYZA vector length, so the real wire speed differs from F on a tapered cut. Prefer CAM that emits **G93**. Exception: hoWiGS's author prefers G94 because live feed override doesn't work in G93.

---

## 4. Recommended stacks

### A. Best free, fastest to good wings: **WiHoWi → FluidNC (SD card or hoWiGS) → devSim check**
- **Pros:** Free, wing-specific, active in 2026, has every feature in §1 except editable washout. The author answers on MakerForums.
- **Cons:** Closed source and alpha, with one developer. If they stop, you're stuck. Windows only. You make root/tip DXFs elsewhere (QCAD, LibreCAD, Fusion, Profili). Confirm tower projection on a cheap test block before relying on it.

### B. Fully open source: **FreeCAD + FoamCut WB → FluidNC WebUI / UGS**
- **Pros:** LGPL, scriptable in Python, cross-platform, active (Aug 2026). G93 by default. Paths are built on the machine side planes, so projection is correct by design. Kerf per object. A real 3D model makes fuselages, nacelles and odd shapes possible.
- **Cons:** Steepest learning curve. No wing wizard. You loft sweep, washout and spars yourself. Expect bugs ("many limitations").

### C. Paid shortcut (~€150–200): **Profili (airfoils) → devWing Foam 2 → GRBL HotWire or FluidNC**
The most complete feature set, including sheeting and multi-panel layout, and the tool most builders already use. If B frustrates you, this is the cheapest way to save time. Gemini Aero Foam (~€40) is the budget paid choice.

### D. Fallback if a GUI tool lets you down
About 200 lines of Python: resample `.dat` files with LE/TE/spar sync points, interpolate airfoil and twist per panel, offset by `kerf(v)` (calibrate with a 10 mm test block at 3 feeds, per side), project with the §1 formula, write G93. Borrow from winggcode and Hot_wire_QT.

---

## 5. Advice specific to a 4 m wing
- Cut **0.8–1.0 m panels**. That needs a ≥1.2 m wire between towers, and the projection maths becomes essential, because the block often sits off-centre.
- Define the whole wing once (span stations with chord, airfoil, twist, sweep, dihedral). Derive each panel's root and tip from it. Neither WiHoWi nor FoamCut does this automatically (devWing and PHOENIX do), so keep a spreadsheet or a script as the master definition.
- Entry and exit at TE height, through the side of the block, keep the beds whole for vacuum bagging the sheeting.
- Typical kerf in EPS 20–30 g/l is 0.3–1.2 mm depending on speed and heat. Calibrate it for every foam batch.
