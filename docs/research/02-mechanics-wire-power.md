# 02: Mechanics, Wire, Heat and Tension (wing cores up to 4 m span)

*Research date 2026-11-03. Prices are rough street prices (AliExpress/eBay/maker shops), not quotes. Physics numbers marked "calc" are my own; formulas are in the notes.*

## TL;DR verdict

**Do not build a 4 m wire. Cut panels of at most 1.25 m and join them.** Use a frame with **two towers on a shared base, spaced about 1.6 m apart (adjustable 0.8–1.8 m)**. Put **GT2 belts on X**, **T8×2 leadscrews on Y**, and **MGN12 or V-slot guides**. Use **0.3–0.4 mm NiCr80** (or **0.3 mm spring-hard 304** for more tension), held by **dead-weight or constant-force-spring tension** on one tower. Feed the wire from a **36–48 V current-limited supply**.

Why:
1. Lag grows with L²: a 4 m wire lags ~6× more than a 1.6 m wire at equal tension, and NiCr can't take much more tension.
2. A 4 m bare wire needs ~50–95 V, mostly above the 60 V DC SELV line, on a touchable wire.
3. Foam comes in short sizes. XPS (Styrodur 3035 CS) is **1250 × 600 mm** usable ([BayWa datasheet](https://res.cloudinary.com/baywa-ag-p/raw/upload/A2809390.pdf), [baustoffshop](https://www.baustoffshop.de/styrodur-3035-cs-hartschaumplatte-1265-x-615-mm.html?p=3)).
4. A 4 m glider needs a 3–4 piece wing for transport/polyhedral anyway, and panels allow per-panel airfoil and taper (4-axis software only blends linearly root→tip).
5. Kerf varies with speed, and root/tip carriage speeds scale with chord ratio. A one-piece 3:1 taper gives bad kerf mismatch; panels at ~1.3:1 are easy to compensate.

Field evidence: RC-Network builders prefer short cuts ("shorter wire sags less, more accuracy", [rc-network.de](https://www.rc-network.de/threads/sinnvoller-arbeitsbereichgr%C3%B6sse-styro-schneideanlage.24844/)). One builder topped out at about 32" (0.8 m) because of centre lag on a spring bow ([Seabreeze](https://www.seabreeze.com.au/forums/Land-Yacht-Sailing/Construction/Hot-Wire-Cutters)). A commercial maker: **at 3 m, spring tension is "almost impossible"**; they use pneumatic tensioners (5–6 bar) and titanium-alloy wire ([hotwiresystems.com](https://hotwiresystems.com/?p=5301)). LMA members do cut 8 ft panels on an 8.5 ft bow, but powered from a mains variac/welder at ~50 V ([LMA forum](https://forum.largemodelassociation.com/viewtopic.php?p=48652)): low precision, unsafe.

## Wire physics (calc)

**Self-weight sag** is s = wL²/8T. For 0.4 mm NiCr (1.06 g/m) it is negligible: 0.09 mm at 1.2 m/20 N and 0.7 mm at 4 m/30 N.
**Lag**, the drag from melting foam, is what matters. It is δ ≈ fL²/8T, with drag f ≈ 0.05–0.15 N/m (at the right speed vs. slightly too fast).

| Wire span | Tension | Lag at f=0.05 N/m | Lag at f=0.15 N/m |
|---|---|---|---|
| 1.2 m | 20 N | 0.5 mm | 1.4 mm |
| 1.6 m | 25 N | 0.6 mm | 1.9 mm |
| 2.0 m | 30 N | 0.8 mm | 2.5 mm |
| 4.0 m | 30 N (NiCr limit) | 3.3 mm | **10 mm** |
| 4.0 m | 60 N (spring 304) | 1.7 mm | 5 mm |

Constant lag is harmless; changing lag (LE/TE turnarounds, speed changes) makes the wire middle cut a different profile from the ends: blunt TE, flattened LE. Mitigate by cutting slowly (2–3 mm/s, which rcKeith uses at about 2.2 A for 750 mm of 0.4 mm NiCr; [FAQ](https://rckeith.co.uk/hot-wire-cnc-faq/)) plus LE/TE dwell. Each 2× length costs 4× lag.

**Tension limit.** NiCr80 0.4 mm (0.126 mm²) creeps hot above roughly 150–250 MPa, so the practical maximum is about 20–30 N (2–3 kg). One builder runs 2 kg on 0.28 mm, and 1300 mm of wire grew about 18 mm from cold to hot ([BARCS forum](https://www.barcs.co.uk/forums/topic/7809-simple-diy-foam-cutter/)). Spring-hard 304 (UTS about 1.8–2 GPa) takes 2–3× more tension on the same diameter. Germans use 0.3 mm VA spring wire on a 10 kg gas strut ([rc-network](https://www.rc-network.de/threads/sinnvoller-arbeitsbereichgr%C3%B6sse-styro-schneideanlage.24844/)).

**Length change the tensioner must absorb.** Thermal growth is about 0.3% hot plus creep. 4-axis geometry adds √(L²+Δx²)−L. With 1.6 m towers and 300 mm root/tip offset (sweep/taper) that is **28 mm**; at 4 m it is 11 mm. So the tensioner needs ≥40 mm stroke at near-constant force: short stiff springs fail; weights, constant-force or long soft springs pass. One vendor says geometry change can reach 150 mm on big 4-axis machines ([cnc-multitool.de product pages](https://www.cnc-multitool.de/cnc-heissdraht-schneidemaschine-einsteiger-serie-cut2500s.html)).

## Wire alloys, resistance and power (calc)

Power per metre for EPS/XPS is about **25–45 W/m**. ThomasHeb uses about 25 W/m ([GitHub](https://github.com/ThomasHeb/4AxisFoamCutter)). rcKeith's 2.2 A on 0.75 m of 0.4 mm NiCr works out to about 40 W/m. A Hackaday commenter's rule of thumb is about 1 W/inch ([Hackaday 2026](https://hackaday.com/2026/07/12/building-a-better-cnc-hot-wire-foam-cutter/)). The needed **current depends only on diameter**. **Voltage scales with length.**

Resistivity used: NiCr80 1.09 µΩm, flat tempco ([Wikipedia](https://en.wikipedia.org/wiki/Nichrome)), matches a supplier's 5.55 Ω/m at 0.5 mm. 304 is 0.73 µΩm cold and about 0.88 hot. Kanthal A1 is 1.45 µΩm ([Kanthal](https://kanthal.com/knowledge-hub/heating-material-knowledge/dimensions-and-properties2/wire)).

| Wire | Ω/m (cold) | Current | 1.2 m V / W | 2 m V / W | 4 m V / W |
|---|---|---|---|---|---|
| NiCr80 0.3 | 15.4 | 1.35 A | 25 V / 34 W | 42 V / 56 W | **83 V** / 112 W |
| NiCr80 0.4 | 8.7 | 2.0 A | 21 V / 42 W | 35 V / 70 W | **70 V** / 140 W |
| NiCr80 0.5 | 5.6 | 2.7 A | 18 V / 48 W | 30 V / 80 W | 60 V / 160 W |
| NiCr80 0.6 | 3.9 | 3.4 A | 16 V / 54 W | 26 V / 90 W | 53 V / 180 W |
| 304 0.3 (hot ≈12.4) | 10.3 | 1.5 A | 22 V / 34 W | 37 V / 56 W | **75 V** / 112 W |
| 304 0.4 (hot ≈7.0) | 5.8 | 2.2 A | 19 V / 42 W | 31 V / 70 W | **63 V** / 140 W |
| 304 0.5 (hot ≈4.5) | 3.7 | 3.0 A | 16 V / 48 W | 27 V / 80 W | 54 V / 160 W |
| Kanthal A1 0.3 | 20.5 | 1.15 A | 29 V / 34 W | 48 V / 56 W | **96 V** / 112 W |
| Kanthal A1 0.4 | 11.5 | 1.75 A | 24 V / 42 W | 40 V / 70 W | **81 V** / 140 W |
| Kanthal A1 0.5 | 7.4 | 2.3 A | 21 V / 48 W | 34 V / 80 W | 69 V / 160 W |

Bold means above the 60 V DC SELV limit. **This is why rcKeith says to go to 24 V electronics above 1 m** ([parts list](https://rckeith.co.uk/parts-list-cnc-foam-cutter/)). For 1.6 m wires you need 30–40 V, which means **a separate wire supply**, not the 24 V board MOSFET.

| Alloy | Pros | Cons |
|---|---|---|
| **NiCr80** | Flat tempco gives even heat and predictable current. Cheap (about $6–10 / €6–10 per 10–30 m). The rcKeith default is 0.4 mm. | Low hot strength and creeps, so it sags and lags on long wires. Some builders report it kinking. |
| **304/316 spring-hard** | 2–3× the tension, so less lag. Cheap (about $8–12 for 50 m of 0.3 mm). | Positive tempco: segments that run hot (outside the foam) get hotter still, giving hot spots ([AAC](https://forum.allaboutcircuits.com/threads/hot-wire-foam-cutter.159063/post-1384055)). Use a **current-regulated** supply and recalibrate kerf. Loses spring temper if overheated. |
| **Kanthal A1** | Highest resistance, so lowest current. Easy to buy (vape wire). | Needs the highest voltage. Strength similar to NiCr, gets brittle after overheating. No real advantage here. |

**Pick:** 0.4 mm NiCr80 to start (most forgiving, best documented). Move to 0.3 mm spring 304 when you want thinner kerf and more tension. Skip 0.5–0.6 mm: kerf is too wide for thin glider TEs.
**Wire PSU:** Mean Well LRS-150-36 (~$25 / €25) for spans up to 1.8 m. For a regulated setup, use a 48 V PSU plus a Riden RD6006 (~$60–80 / €60–75) in constant-current mode, PWM'd or set by the controller. Never use AC or a variac. AC can cause wavy cuts and is a shock hazard.

## Tensioning options

| Method | Verdict |
|---|---|
| **Bow** (rcKeith: softwood 25×13 mm, expansion spring, wire sits in V-grooves on the carriages; [bow page](https://rckeith.co.uk/hot-wire-bow-for-foam-cutting/)) | Best for **≤1.2 m**: no load on the carriages, so cheap drawer-slide towers work. At 1.6–2 m the bow gets heavy and floppy and loads the Y axes. |
| **Short extension spring at the tower** | Cheapest, but tension varies ±30% over a 28 mm stroke. OK only for short, untapered cuts. |
| **Dead weight over a pulley** | **Constant tension**, cheapest, absorbs any stroke. A forum 4-axis builder reports a ~5 kg weight on a rail-guided insulator "works a treat" (unverified anecdote). It sways during fast rapids, so damp it with a tube. |
| **Constant-force (clock) spring**, 20–30 N, ~$8–15 | **My pick**: constant, compact, no sway. Mount it on one carriage, wire over a 624 V-groove bearing. |
| Gas strut / pneumatic / servo feedback | For >2.5 m machines only ([hotwiresystems](https://hotwiresystems.com/?p=5301)). Overkill here. |

Important consequence: any tensioner other than a bow pulls the two carriages together with 20–30 N. So **the towers must be tied by a rigid base**, not two loose towers on a table. Wire contact and pivot: use **MIG contact tips** as wire guide and current feed, as in the 2026 Hackaday build ([GitHub MichaelRechtin](https://github.com/MichaelRechtin/CNC-Hotwire-Foam-Cutter)), or a V-groove bearing with the clamp behind it.

## Linear motion and drives

Loads are tiny: drag is under 1 N and speeds are 2–8 mm/s when cutting, about 30 mm/s on rapids. Stiffness only matters against the wire-tension moment.

| Guide | ~Price per 1 m axis | Notes |
|---|---|---|
| Drawer slides (rcKeith) | $8–15 / pair | Cheapest. 0.1–0.3 mm play. Fine with a bow, poor with frame tension. |
| **V-slot 2040/2060 + 4 V-wheels** | $10–18/m extrusion + ~$8 wheels | Frame and rail in one piece. Eccentric adjustment. Flat-spots wheels if parked under load. Good value. ([OpenBuilds parts](https://openbuildspartstore.com/v-slot/)) |
| **MGN12H clone** on 2020/2040 | $26–40/m ([Makerstore](https://makerstore.cc/product-category/linear-rail/), [Robotshop $66](https://reach.dog/shop/robotshop-usa/products/3d-printing-canada-usa-3d-printing-canada-12mm-linear-rail-1000mm-long-with-mgn12h-block)) | Stiffest, no adjustment. Best Y axis (resists the tension moment). |
| Smooth rod Ø12 | $10–15/m | 1 m rod on end supports sags about 0.5 mm under its own weight (calc). Avoid for X. SBR16 supported rail is OK. |

| Drive | Resolution @1/16 µstep | Pros | Cons |
|---|---|---|---|
| **GT2 6 mm, 20T** (40 mm/rev) | 80 steps/mm, 12.5 µm | Cheap ($1–2/m), any length, no whip, used by ThomasHeb (80 steps/mm) | About 0.05–0.1 mm backlash/stretch. Vertical axis drops when unpowered, and the wire cuts into the core. |
| **T8×2 leadscrew** | 1600 steps/mm | Self-locking: Y holds when powered off. Precise. 400 mm kit with nut is ~$10–15 on AliExpress, $30–44 retail ([Abra](https://abra-electronics.com/3d-printers-en/linear-movement/kits/mp-t8-400mm-t8-stainless-steel-lead-screw-with-nema-17-stepper-motor-brass-nut-and-shaft-coupler-kit.html)) | ~20 mm/s at 600 rpm (fine for Y). T8×8 is faster but back-drives and whips at 1 m. |
| M10×1.5 threaded rod (rcKeith) | 2133 steps/mm | Dirt cheap | Slow, wobble, nut backlash. |

Both drives are far finer than kerf scatter (±0.1–0.2 mm), so backlash compensation and kerf calibration matter more.
**Pick: X = GT2 belt, Y = T8×2 leadscrew.** Motors: 4× NEMA17 1.5–2 A (~$10–14 each).

## Reference designs

- **rcKeith USB foam cutter**: MDF, drawer slides, M10 rod, bow, 0.4 mm NiCr, GRBL Hotwire. About $200 total, plans $3 ([rckeith.co.uk](https://rckeith.co.uk/), [eBook](https://rckeith.co.uk/how-to-build-a-usb-cnc-hot-wire-foam-cutter/)). Proven and cheap, limited to about 1 m.
- **ThomasHeb 4AxisFoamCutter**: 2020 frame, V-slot, GT2, retractable-reel tension, 0.75–0.9 m wire ([GitHub](https://github.com/ThomasHeb/4AxisFoamCutter)).
- **MichaelRechtin (Hackaday, Jul 2026)**: 4040 folding frame, 16 V-wheels, 700 mm X / 400 mm Y leadscrews, spring lanyard, MIG tips ([GitHub](https://github.com/MichaelRechtin/CNC-Hotwire-Foam-Cutter), [Hackaday](https://hackaday.com/2026/07/12/building-a-better-cnc-hot-wire-foam-cutter/)).
- **Jedicut**: free 4-axis software, with a 3D-printed DIY machine and forum ([jedicut.com](https://www.jedicut.com/en/)).
- Others: [Instructables 4-axis RAMPS](https://www.instructables.com/4-Axis-CNC-Hot-wire-Foam-Cutter-ArduinoRamps14), [How To Mechatronics](https://howtomechatronics.com/?p=6047) (2-axis), [OpenBuilds foam builds](https://builds.openbuilds.com/?tag=foam), [UWARG GRBL-Hotwire+Jedicut wiki](https://uwarg-docs.atlassian.net/wiki/spaces/MEC/pages/3865509890/Using+the+CNC+Hot+Wire+Cutter).

## Recommended machine ("1.6 m panel cutter")

| Item | Spec |
|---|---|
| Max core length | **1250 mm** (one XPS board). A 4 m wing is 2 × ~1.25 m centre panels plus 2 × ~0.75 m tips, or 4 panels. |
| Tower spacing | 1600 mm default, adjustable 800–1800 mm by sliding the towers on the base |
| X travel | **1000 mm** (chord up to 400 mm + sweep/offset up to 300 mm + run-in/out) |
| Y travel | **300 mm** (foam up to 150 mm thick + clearance and dihedral offset) |
| Base | 2 × 2040 (or 2060) rails, 1900 mm long, joined by cross-members. Carries the wire-tension loop. Table top 1300 × 600 mm MDF or melamine. |
| X axes | 2 × 2040 × 1100 mm with V-wheel plates (budget) or MGN12H 1000 mm. GT2 6 mm belt, 20T pulley + idler. |
| Y axes | 2 × 2020 × 450 mm + MGN12H 400 mm rail, T8×2 × 400 mm leadscrew, motor on top |
| Wire | 0.4 mm NiCr80 (later 0.3 mm spring 304), about 1.9 m incl. tails |
| Tension | One side fixed clamp, other side 624 V-bearing + 25 N constant-force spring (or 2.5 kg weight in a tube), stroke ≥60 mm |
| Wire power | 36–48 V PSU → CC module, about 2 A × 30 V ≈ 60–70 W at 1.6 m |
| Est. mechanical BOM | **~$180–260 / €170–240** (V-slot version), plus about $50–100 for MGN12 everywhere. Electronics are covered in other notes. |

Upgrade path: if you later want 2 m panels, slide the towers to 1.8 m, switch to 0.3 mm spring 304 at about 40 N, and raise the supply to 48 V. Don't go beyond that.
