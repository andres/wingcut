# 04 — Kits, reference builds & cost benchmarks: 4-axis CNC hot-wire cutters

*Researched 2026-11-03. "≈" prices are typical AliExpress/eBay street prices, not quotes. EUR ≈ USD × 0.92.*

## TL;DR (opinionated)
- **No sensible hobby 4-axis kit is sold today.** Commercial machines start around **$3.5k–5k** (China) and go up to **$11k–17k** (US). Below that, the market is DIY plans plus 3D-printer electronics. **Build it.**
- **A 4 m span does NOT mean a 4 m wire.** Cut the wing in **panels of 0.8–1.25 m** and join them (you need joiners/spars anyway). Design for a **~1.3–1.5 m wire**, **≥1.0 m chord travel** and **~0.3–0.4 m vertical travel**. A longer wire makes sag/lag much worse (see gotchas).
- **Hardware is the easy part.** The quality of your cores depends on wire temperature control, tension, kerf/lag compensation and templates/software. Spend money there first.
- **Sweet spot: the mid tier (~$450–650).** It has 2040 V-slot, NEMA17 + TMC2209, a GRBL/FluidNC 4-axis board, a separate PWM'd wire supply and a constant-tension bow.

## 1. Commercial machines & kits (benchmark)

| Vendor / product | Type | Size | Price | Verdict |
|---|---|---|---|---|
| Hot Wire Foam Factory **Aero 4-axis** ([site](https://hotwirefoamfactory.com/cnc)) | Turnkey, Mach3 | 48″ wire, 60×44×56″ footprint | **$11,000** (≈€10.1k) | Proven but 20× DIY cost |
| HWFF **XL 4-axis** | Turnkey | 102″ wire × 60″ × 58″ | **$17,400** | Sign/architectural industry |
| Alibaba "WISE" 4-axis EPS CNC ([listing](https://www.alibaba.com/product-introduction/4-axis-hot-wire-eps-cnc_1600259659485.html)) | Industrial, freight | 1–2 m+ | **$4,700–5,300** + shipping/VAT | Overkill, building-industry software |
| Alibaba 4-axis + 3-axis wire ([listing](https://www.alibaba.com/product-introduction/3-axis-4-axis-hot-wire_1601122940297.html)) | Industrial | — | **$4,250–4,500** | Same |
| eBay new generic CNC hot-wire ([search](https://www.ebay.com/sch/i.html?_nkw=hot+wire+cnc)) | Turnkey | unknown | ~$3,550 | Unknown controller/software |
| WindyHill Hobbies kit ([Tindie](https://www.tindie.com/products/windyhillhobbies/hotwire-cnc-machine-kit/)) | Mechanical kit, 3D-printed brackets | 40×40×12″ | **$549**, *no* motors/electronics/bow/wire; seller "on break" | Too pricey for what's included |
| Vortex-RC kit (India, [rcindia 2016](https://rcindia.org/rc-general-topics/4-axis-diy-hot-wire-cnc-kit-for-cutting-foam-airfoils-and-fuselages/msg321142)) | Laser-cut kit, Mach3 | — | ₹15,500 (~$185) without electronics | Dated, availability unclear |
| HobbyCC FoamPro electronics kit (used on [Unistrut build](https://hackaday.com/2014/10/06/4-axis-cnc-foam-cutter-sports-a-unistrut-frame)) | 4× NEMA23 + board | — | historical | Legacy parallel-port era |
| **rcKeith** ([rckeith.co.uk](https://www.rckeith.co.uk/)) | **Plans $3 (DXF/PDF), free eBook**, no kit | ~1 m bow | ~$200 parts claim ([eBook](https://www.scribd.com/document/718326434/eBook-6)) | **Best documented hobby design.** Covers MKS TinyBee, RAMPS, MKS Gen L, SKR Pico, GRBL HotWire Mega 5X |
| CNC4ALL, Lightobject, "Hotwire Direct", "VJ foam cutters" | — | — | **No current 4-axis listing found** | Treat as unavailable; don't plan around them |
| AliExpress "4-axis foam cutter kit" | — | — | **No coherent kit found.** You get 3D-printer/CNC parts bundles only | Buy the parts separately |

## 2. Reference DIY builds (copy from these)

| Build | Mechanics | Electronics / firmware | Cost | Notes |
|---|---|---|---|---|
| **rcKeith USB cutter** ([OpenBuilds](https://openbuilds.com/builds/usb-hot-wire-cnc-foam-cutter.8770/), [Flite Test](https://www.flitetest.com/articles/Hot_Wire_CNC_foam_cutter)) | Drawer slides + threaded rod (newer: printed parts) | RAMPS/MKS/SKR Pico, GRBL HotWire Mega 5X | **~$200 / £160 / €180** | Huge community; [bow design](https://rckeith.co.uk/hot-wire-bow-for-foam-cutting/) with 0.4 mm nichrome, spring + MIG-wire tensioner |
| **c-devine WingGcodeBuilder** ([wiki](https://github.com/c-devine/WingGcodeBuilder/wiki)) | 2020 extrusion, 8 mm rods + LM8UU, GT2, constant-force spring | MKS Gen 1.4, modified Marlin | **$214 itemised** | Only fully priced BOM found; small (500 mm rods) |
| **ThomasHeb 4AxisFoamCutter** ([GitHub](https://github.com/ThomasHeb/4AxisFoamCutter)) | 2020 V-slot, belts, 80 steps/mm | Mega + RAMPS 1.4/1.6, A4988, grbl 8c2 fork with **wire power % (M3 Sxx)**, LCD + SD | not stated | Best wire-sizing method (below); Kanthal A1 32G, 0.9 m at 30 V; cuts XPS ~100 mm/min |
| **Michael Rechtin (2026)** ([Hackaday](https://hackaday.com/2026/07/12/building-a-better-cnc-hot-wire-foam-cutter/), [Arduino blog](https://blog.arduino.cc/2026/07/16/this-cnc-hot-wire-cutter-was-created-specifically-for-making-airplane-wings/)) | Extrusion, printed parts, foldable, **pulley constant-tension**, **MIG tips as current feed** | Mega + RAMPS 1.4 + TMC2209, variable DC PSU | not stated (~$250–350 est.) | Most modern design; repo `MichaelRechtin/CNC-Hotwire-Foam-Cutter` |
| **FluidNC foam board** ([MakerForums](https://forum.makerforums.info/t/fluidnc-4-axis-foam-cutter-controller/91880)) | — | ESP32 + 4 Pololu-style drivers, FluidNC, GRBLHotwire/Jedicut | ESP32 + drivers + 4 motors ≈ **€31** | Modern, Wi-Fi capable. Watch the CH340 driver issue below |
| jpralves RAMPS build ([blog](https://jpralves.net/post/2018/09/27/4-axis-cnc-hot-wire-foam-cutter-arduinoramps14.html)), Make "hardware store" build ([Make](https://makezine.com/article/technology/arduino/building-a-four-axis-foam-cutter-from-hardware-store-parts/)), Unistrut 48×42″ (Hackaday above), How To Mechatronics ([2-axis](https://howtomechatronics.com/projects/arduino-cnc-foam-cutting-machine/), not 4-axis) | various | Arduino + CNC shield | $150–400 | Useful as references only |

**Free software:** [Hot_wire_QT](https://github.com/mstrens/Hot_wire_QT) (Python GUI: .dat root/tip airfoils, sync points, spar/aileron cuts, drives grbl-Mega-5X directly), [Jedicut](https://www.jedicut.com/) (free), GRBL HotWire, rcKeith Wing Designer, GMFC (free variant). Commercial: DevWing Foam + Mach3 was quoted at "up to £300/$400" ([OpenBuilds](https://builds.openbuilds.com/threads/using-free-software-to-make-foam-wings.12398/)). **Don't pay for software.** The free stack is enough for straight-tapered and swept panels.

## 3. Cost tiers (for ~1.3–1.5 m wire, ≥1 m travel)

| Category | **Ultra-cheap $150–250** | **Mid $400–700** (recommended) | **Robust $1,000+** |
|---|---|---|---|
| Frame | Plywood/MDF + drawer slides or 8 mm rods; 0.8–1.0 m wire max | 2040 V-slot (~$8–12/m, ~8 m ≈ $80), printed/ply plates | 4040/4080 extrusion or welded steel, ~$200–300 |
| Linear motion | Drawer slides / LM8UU on rods ($30–50) | V-wheels on V-slot or MGN12 rails ($60–120) | HGR15/HGH15 rails, 4 axes (~$200–350) |
| Drive | M8 threaded rod or GT2 6 mm ($15) | GT2 9–10 mm belts or T8 lead screws on Z ($40) | SFU1605 ball screws or 15 mm HTD belts ($150–250) |
| Motors | 4× NEMA17 40 mm ($35) | 4× NEMA17 48 mm 2 A ($45–60) | 4× NEMA23 or closed-loop 57 ($150–300) |
| Controller | Mega + RAMPS 1.4 clone + A4988 ($25) | SKR Pico / MKS TinyBee / FluidNC ESP32 + **TMC2209** ($30–50) | FluidNC 6-pack + DM542 externals ($150–200) |
| Motor PSU | 12 V 10 A ($15) | Mean Well LRS-350-24 ($35, ≈€33) | 36–48 V PSU ($60–100) |
| Wire power | Hobby charger / car charger / old ATX ($0–30) | Separate 24–36 V PSU + **PWM MOSFET from spindle pin** ($30–50) | **Constant-current** or closed-loop (measure I, regulate) + 0.1 A meter ($80–150) |
| Wire & bow | 0.3–0.4 mm nichrome 10 m ($8), wood bow + spring | Nichrome/Kanthal + **constant-tension bow** (pulley/constant-force spring) ($25) | Spring/pneumatic tensioner + MIG-tip feeds, spare reels ($50) |
| Homing/safety | none | 4 endstops + E-stop + fuse ($15) | Inductive sensors, E-stop relay cutting wire power ($40) |
| Bed / jigs | Floor board | Flat 18 mm ply/MDF, sealed + block clamps ($40) | Aluminium tooling plate or torsion-box bed ($100+) |
| Fumes | Open window | Fan + duct out ($30–50) | Downdraft/extraction hood ($100–200) |
| Software | Free | Free | Free (or DevWing ~$150–400 if you want it) |
| **Total** | **~$150–250 / €140–230** | **~$450–650 / €420–600** | **~$1,000–1,800 / €0.9–1.7k** |

**Where the money goes:** In the cheap tier, ~40 % goes on electronics and motors. In the mid tier, ~45 % goes on frame, extrusion and linear motion. In the robust tier, the rails and screws plus big motors and drivers make up ~60 %. **Wire temperature control costs little in every tier and has the biggest effect on core quality.**

## 4. Gotchas builders actually hit

| Problem | Cause | Fix |
|---|---|---|
| **Wire lag → dished LE, scalloped/thin TE** | Wire bows backward under drag. Worse with long wire, fast feed, cold wire ([RCU](https://www.rcuniverse.com/forum/airplanes-all-types-286/11715658-irregular-wing-surface-ridges.html)) | **Start the cut at the LE.** Slow down at the LE radius. Raise the tension. Leave TE ~1–1.5 mm thick and finish after skinning |
| **Burned/wide kerf at the root or tip side** | On a tapered wing the tip moves slower, so it melts more ([Jedicut forum](https://www.jedicut.com/forums/viewtopic.php?p=8993)) | Use per-side kerf compensation (Jedicut/GRBL HotWire support it), or feed-rate coupling. Kerf wider than the wire diameter means the wire is too hot |
| **Sag on long wires** | Thermal expansion. Nichrome sags noticeably above ~1.5 m ([source](https://forum.allaboutcircuits.com/threads/hot-wire-foam-cutter.159063/)) | Use an **automatic tensioner** (spring/pulley). Keep the wire ≤1.5 m. Use thicker wire only with more tension |
| **Wire snaps** | Spring between carriages overloads them at big sweep angles ([rcKeith](https://rckeith.co.uk/hot-wire-bow-for-foam-cutting/)). Hot spots at the clips | Use a **bow** or pulley tensioner, not a carriage-to-carriage spring. Use MIG contact tips instead of crocodile clips. **Wear eye protection** ([Petaurus](https://petaurusaero.substack.com/p/hot-wire-cutting-wings)) |
| **Temperature drift** | R changes with active length (diagonal cuts) and with temperature ([Hackaday comments](https://hackaday.com/2026/07/12/building-a-better-cnc-hot-wire-foam-cutter/)) | Use constant current, not constant voltage. Size for **~25 W/m**: P = L·25, R = U²/P ([ThomasHeb](https://github.com/ThomasHeb/4AxisFoamCutter)). For 1.4 m that is ~35 W, e.g. 24 V/1.5 A |
| **Too cold / slow / choppy cut** | Weak supply (ATX 12 V), template friction ([RCU thread](https://www.rcuniverse.com/forum/questions-answers-154/11635059-struggling-my-home-built-hot-wire-foam-cutter.html)) | Add more current and tension. The right heat leaves fine "hair" strings with no drag |
| **Block moves or drops into the kerf** | Gravity-only hold-down ([Hackaday](https://hackaday.com/2014/10/06/4-axis-cnc-foam-cutter-sports-a-unistrut-frame)) | Weight the block (~10 kg) or pin it. Glue or tape small blocks to a sacrificial base |
| **Twisted (washout-error) panels** | Block not square to the axes. Root/tip zero mismatch | Square the block against fence stops. Home both towers. Run a test cut on a cheap block |
| **Foam quality** | Pink/blue XPS cuts fine but is heavier. Low-density EPS beads give a rough surface | Use **EPS 20–30 kg/m³** or XPS for cores. **Never hot-wire PIR/PU** (toxic fumes) |
| **Controller silently fails** | A Windows update broke the CH340 USB driver ([MakerForums](https://forum.makerforums.info/t/fluidnc-4-axis-foam-cutter-controller/91880)) | Reinstall the older CH340 driver, or use an ESP32/Wi-Fi or native-USB board |
| **RAMPS quirks** | 12 V max for the Mega regulator, limit-switch EMI, homing hard-errors ([ThomasHeb](https://github.com/ThomasHeb/4AxisFoamCutter)) | Use a separate wire PSU. Put 100 nF caps on the switches. Commission with homing off |
| **Fumes** | Styrene/irritants. Polystyrene ignites around 360 °C if the wire is overdriven ([guide](https://gearprism.pages.dev/makers/hot-wire-foam-cutter/)) | Run the wire at 150–250 °C. Use real extraction, not a desk fan. Keep an extinguisher nearby. Never leave the cutter unattended |

## 5. Recommendation for a ≤4 m UAV/glider wing
Build the **mid tier**. Base it on the rcKeith/Rechtin layout: 2040 V-slot, 1.5 m between towers, 1.1 m chord travel, 0.35 m Z, NEMA17 + TMC2209, FluidNC ESP32 or SKR Pico running GRBL HotWire Mega 5X, a dedicated 24–36 V wire PSU with PWM, a pulley constant-tension bow, and MIG-tip feeds. Use free software (Hot_wire_QT / Jedicut / GRBL HotWire). Cut 4–5 panels of 0.8–1.0 m per wing. **Budget ~$550 (≈€500) plus ~$50 of foam and wire for tuning.** Upgrade to rails or constant-current wire control only once lag and kerf are dialled in.
