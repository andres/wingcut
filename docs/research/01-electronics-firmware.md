# 01 — Controller electronics + firmware for a 4-axis (XY + UV) hot-wire cutter

Scope: a DIY CNC hot-wire cutter for EPS/XPS wing cores, up to 4 m total span. That means cutting panels of about 1–1.3 m with a bow of about 1.4–2 m. Prices are approximate, from Nov 2026 marketplace and store pages. Conversion used: 1 USD ≈ 0.87 EUR. Check live listings before buying.

## TL;DR (opinionated)

| | Stack | Electronics cost* | Verdict |
|---|---|---|---|
| **Primary** | **BTT SKR Pico V1.0 + grblHAL ("SKR Pico Hotwire" build) + GRBL-HotWire PC software** | ~$35–60 / €30–52 | Best value. 32-bit, 4× TMC2209 on board with current set in software, an official hot-wire board map, and support from the main hobby hot-wire software. |
| Alt A | ESP32 + **FluidNC** (Bart Dring 4x TMC2209 board, or 6-Pack + external drivers) | ~$50–70 / €45–60 board | YAML config, no compiling, WiFi WebUI. Board stock is patchy. |
| Alt B | **Arduino Mega 2560 + RAMPS 1.4 + DRV8825**, flashed with GRBL HotWire Mega-5X *or* Marlin `FOAMCUTTER_XYUV` | ~$35–90 / €30–78 | Cheapest and best documented (rckeith). Marlin version runs standalone from SD with no PC. 8-bit, noisy drivers. |
| Pro / overkill | LinuxCNC + Mesa 7i96S + DM542T + NEMA23 | ~$300+ | Native XYUV. Only worth it if you already run LinuxCNC. |
| Avoid | Klipper; Uno + CNC Shield V3; Mach3/4 | — | Reasons below. |

*Board + drivers only. Motors, PSU and wire supply are extra; see BOM below.

## Why feed rate on 4 axes matters (read this first)

On a tapered wing the root and tip planes travel different distances in the same time. The firmware has to know which plane's speed to honour. Otherwise the slower plane over-burns and the kerf varies.

- **G93 inverse-time feed** fixes this cleanly. The CAM gives each move a duration, so the speed choice no longer depends on the firmware. grbl, grblHAL and FluidNC support G93, and GRBL-HotWire supports G93 mode. rckeith advises: "Adjust Heat, Not Speed" ([rckeith](https://rckeith.co.uk/grbl-hotwire-mega-5x-for-cnc-foam-cutters/)).
- **Marlin `FOAMCUTTER_XYUV`** maps the axes directly with no kinematics. Its feed-rate handling is said to beat generic multi-axis hacks ([DerAndere1 issue](https://www.github.com/DerAndere1/Marlin/issues/65)). Marlin has no G93.
- **LinuxCNC** and **Klipper** compute feed rate from XYZ only, so UV just follows. That's fine if the CAM writes XY as the larger (root) plane.

## Firmware options

| Firmware | 4-axis hot-wire status (2026) | Pros | Cons |
|---|---|---|---|
| **GRBL HotWire Mega-5X** (rckeith fork of grbl-Mega-5X 1.2) | Axes **XYZA**; older firmware used XYZU. One build for Mega/RAMPS/MKS Gen L. Updated **Apr 2026**. 1/8 µstep, default 1066 steps/mm for M10×1.5 rod. Min limits only ([rckeith](https://rckeith.co.uk/grbl-hotwire-mega-5x-for-cnc-foam-cutters/)) | Purpose-built and free. Pairs with GRBL-HotWire software (2D/4D cut preview, jog planes together or apart, G93) | 8-bit AVR. The PC software is Windows-only |
| **grblHAL** | Has a dedicated **`BOARD_BTT_SKR_PICO_10_HOTWIRE`** map. Spindle PWM goes to the heated-bed MOSFET (GPIO21) ([board map](https://github.com/grblHAL/RP2040/blob/master/boards/btt_skr_pico_10_map.h)). Ganged axes and up to 8-axis maps exist ([RP2040 boards](https://github.com/grblHAL/RP2040/tree/master/boards)). Web Builder, no toolchain needed. Very active (commits Nov 2026) | 32-bit, high step rates, TMC2209 UART current via `$140–$143`. GRBL-HotWire supports grblHAL. Teensy 4.1 boards go up to 5 axes ([T41U5XBB](https://www.tindie.com/products/philba/grblhal-bob-unkit-for-teensy-41-t41u5xbb/)) | You must choose the right board/options in the Web Builder |
| **FluidNC** (ESP32) | Axis table goes X Y Z A B C U V W ([source](https://github.com/bdring/FluidNC/blob/main/FluidNC/src/Machine/Axes.cpp)). Up to 2 motors per axis. Latest **v4.1.1, Sep 2026**. Ready XYZA configs for the 4x_2209 board ([config repo](https://github.com/bdring/fluidnc-config-files/tree/main/contributed/4x_2209)). A working FluidNC foam controller exists: GRBL-HotWire "works very well with FluidNC V3.9.2 PR2 or above", with wire PWM via `s0_with_disable` ([PCBWay project](https://www.pcbway.com/project/shareproject/FluidNC_foam_cutter_controller_bad9d318.html)) | Edit a YAML file, no flashing. WiFi/WebUI and SD streaming | The ESP32 USB-serial (CH340) driver had issues with GRBL-HotWire on Win11 (fix: driver rollback). Fewer hot-wire users than grbl/grblHAL |
| **Marlin 2.1.x `FOAMCUTTER_XYUV`** | In mainline `Configuration.h`: "parallel horizontal axes (X, I) … heights … (Y, J). Joints are directly mapped" ([Configuration.h](https://github.com/MarlinFirmware/Marlin/blob/bugfix-2.1.x/Marlin/Configuration.h)). Rename axes with `AXIS4_NAME 'U'`. rckeith's RAMPS build is the reference. Release 2.1.2.9 (Oct 2026) | Runs **PC-free** from SD + LCD. Runs on almost any 3D-printer board | You compile it yourself. Config has quirks (a fake Z axis, pin redefinitions). No G93 |
| **LinuxCNC 2.9/2.10** | Native XYUV: `COORDINATES = X Y U V`, `GEOMETRY = XY;UV`, `FOAM = 1`. Ships a `configs/sim/axis/foam` sample ([forum](https://forum.linuxcnc.org/38-general-linuxcnc-questions/26106-xyuv-foamcuttter-configuration-from-scratch-help), [repo](https://github.com/LinuxCNC/linuxcnc/tree/master/configs/sim/axis/foam)) | Rock-solid real-time motion and a proper foam preview. Free | Needs a PC with a real-time kernel plus LPT or Mesa ([7i96S **$149**](https://store.mesanet.com/index.php?route=product/product&product_id=374)). Stepconf can't create foam configs. G2/G3 arcs in UV are limited |
| **Klipper** | *Possible*: `MANUAL_STEPPER … GCODE_AXIS=U` turns extra steppers into synchronized G1 axes ([G-Codes.md](https://github.com/Klipper3d/klipper/blob/master/docs/G-Codes.md)) | Nice if you already run a Pi | Needs a Pi host. Feed rate comes from XYZ only. Heat output is not motion-synced. No G93. **Not grbl protocol**, so GRBL-HotWire/Jedicut can't talk to it. **Don't.** |
| **Mach3/Mach4** (closed reference) | Works for XYZA/XYUV. Mach4 Hobby ≈ **$200**, Mach3 ≈ $175–220 ([machsupport](https://www.machsupport.com/licensing/)) | Mature, and some foam CAM targets it | Windows, paid, needs LPT or a motion controller. No advantage over grblHAL at this scale |
| **grbl-xyuv** (Uno) | grbl 0.8 fork for Uno + Protoneer shield ([repo](https://github.com/rasciodc/grbl-xyuv)) | Runs on parts you may already own | Abandoned codebase. Flash and pin limits |

**About the Uno + CNC Shield V3:** with stock grbl, the shield's 4th ("A") driver slot can only *clone* X, Y or Z through jumpers. A true independent 4th axis needs a fork that steals D12/D13, which conflicts with spindle PWM and limit pins. A Mega + RAMPS costs a few dollars more and avoids all of this. **Skip the Uno.**

## Board options

| Board | MCU | Drivers | Firmware fit | Approx. price |
|---|---|---|---|---|
| **BTT SKR Pico V1.0** | RP2040 | 4× TMC2209 on board (UART) | grblHAL hot-wire map, Klipper, rckeith guide | **$36** Amazon (Jun 2026) ([SpecPicks](https://specpicks.com/product/B09MYKL9MP)); resellers $50–62 / ~€35–55 |
| Bart Dring **4x CNC (ESP32+TMC2209)** | ESP32 | 4× TMC2209, 2× 3 A MOSFET outputs, 0–10 V, RS485 | FluidNC | **$49**, often out of stock ([Elecrow](https://www.elecrow.com/4x-cnc-controller-integrated-esp32-and-tmc2209.html)) |
| **6-Pack** (FluidNC) | ESP32 | 6 sockets: StepStick modules or external drivers | FluidNC | ~$64 Tindie / $49 Elecrow (2025) ([CNX](https://www.cnx-software.com/2025/01/30/fluidnc-compatible-cnc-controller-supports-6-stepper-drivers-web-based-configuration/)) |
| FYSETC E4 | ESP32 | 4× TMC2209 | Marlin (FluidNC possible) | ~$29 ([FYSETC](https://fysetc.github.io/E4)) |
| MKS DLC32 V2.1 | ESP32 | **only 3** driver slots | FluidNC/grbl_esp32 | $39–69. **Not 4-axis.** The DLC32 MAX adds an A axis but has poor docs ([forum](https://forum.makerforums.info/t/mks-dlc32-v2-1/88240)) |
| **Mega 2560 + RAMPS 1.4** | AVR | 5 StepStick sockets | GRBL HotWire, Marlin | kit with 5× DRV8825 ~$35–90 / ~€30–79 ([eBay example €79](https://www.ebay.com/itm/255337472018)) |
| MKS Gen L / TinyBee | AVR / ESP32 | 5 / 3+ sockets | rckeith guides | ~$25–40 |
| Teensy 4.1 + T41U5XBB | i.MX RT1062 | external drivers | grblHAL, 5 axes, Ethernet version too | Teensy ~$32 + board ~$30–100. The seller moved off Tindie |

## Stepper drivers and motors

| Driver | Current | Cost each | Use |
|---|---|---|---|
| **TMC2209** (StepStick/on-board) | ~1.2–1.4 A RMS sustained (2 A peak) | $3–6 | **Default.** Silent, current set in software, StallGuard homing possible |
| DRV8825 | ~1.5 A, needs a heatsink | $1.5–3 | OK on RAMPS. Noisy. Set Vref with a potentiometer |
| A4988 | ~1 A RMS | $1–2 | Marginal |
| TB6600 (external) | "4 A", but many clones deliver about half ([guide](https://cncrouterinfo.pages.dev/guides/stepper-driver-comparison)) | $8–12 | Avoid |
| **DM542T** (external) | 1–4.2 A, 20–50 V | ~$25–35 / €23–35 | Only for NEMA23 or long cable runs |

**Motors:** cutting force is close to zero and feeds are about 2–8 mm/s, so **NEMA17, 1.5–2 A, 0.45–0.59 Nm** is plenty. Example: StepperOnline 17HS19-2004S1, about $10–14 each. rckeith uses cheap NEMA17 and says NEMA23 also works ([rckeith](https://rckeith.co.uk/)). Use NEMA23 + DM542T only for heavy, long (>1.5 m) gantries or long belt drives. Run the motors at 24 V. Each TMC2209 output can drive 2 steppers in series for a ganged axis ([Gemini AeroCutter manual](https://www.geminiaerotools.com/files/Cutter/GeminiAeroCutter_UserManual.pdf)).

## Wire heat control

Rough sizing (my estimate): 0.3 mm Ni80Cr20 is about 15 Ω/m.

| Bow length | R (cold) | Typical cutting current | Supply voltage | Power |
|---|---|---|---|---|
| 1.0 m | ~15 Ω | 1.2–1.6 A | 18–24 V | 25–40 W |
| 1.4 m (1.2 m panel) | ~21 Ω | 1.2–1.6 A | 25–34 V | 30–55 W |
| 2.0 m (2 m half-wing) | ~30 Ω | 1.2–1.6 A | 36–48 V | 45–75 W |

Options, in order of preference:

1. **PWM from the spindle/laser output → external logic-level MOSFET module (IRLZ44N or D4184-type) on a *separate* 36–48 V supply.** A Mean Well LRS-350-36 or -48 costs about $40 / €35. Firmware sets heat with `M3 S<n>` / `M5`. Turn **laser mode off** (`$32=0`) so heat stays constant through moves. The proven FluidNC foam board uses this design: a 2N7000 driving a BUZ11, with PWM `low` and `s0_with_disable` ([PCBWay](https://www.pcbway.com/project/shareproject/FluidNC_foam_cutter_controller_bad9d318.html)). Don't run the wire straight off the board's MOSFET at board voltage unless the wire is ≤1 m and the board's PSU has headroom. The SKR Pico hot-wire map puts PWM on the HB MOSFET, so you *can* do it for short wires.
2. **Constant-current supply** (Riden RD6006/RD6012, about $60–90 plus a 48–60 V PSU). Set the current, then switch it with a DC SSR (~$8) on spindle-enable. This gives the most repeatable kerf when you change wire length or tension, because current, not voltage, sets wire temperature. Downside: the heat isn't controlled by G-code unless you add Modbus. Forum opinion is split on CC vs CV ([AAC thread](https://forum.allaboutcircuits.com/threads/hot-wire-cutter.129583/page-2)).
3. Cheap variable buck/boost module plus a manual switch. Fine for a first test, poor for repeatability.

Always fit an **E-stop that cuts the wire supply**, and fuse the wire circuit at about 3 A.

## Recommended stacks with BOM

**Primary: SKR Pico + grblHAL + GRBL-HotWire (≈$150 / €130 excluding frame)**
SKR Pico $36 · 4× NEMA17 2 A $48 · 24 V 6 A PSU $20 · 36 V 10 A wire PSU $40 · MOSFET module + DC SSR/E-stop $12. Flash grblHAL from the Web Builder with the *SKR Pico Hotwire* board and 4 axes. Software: GRBL-HotWire (free, Windows) or any CAM that emits XYZA/G93. rckeith publishes a step-by-step SKR Pico guide.
*Pros:* cheapest 32-bit option, current set in software, quiet, has an explicit hot-wire map, actively maintained.
*Cons:* the drivers are soldered on (a blown driver means a new board), and you need a Windows PC for GRBL-HotWire.

**Alt A: FluidNC 4x TMC2209 ESP32 board (≈$165)**
*Pros:* edit YAML over WiFi without reflashing, web UI and SD card, so you can cut from a phone. GRBL-HotWire v6.4 supports it.
*Cons:* stock is unreliable, there's the CH340 driver issue on Windows, and the hot-wire community is smaller.

**Alt B: Mega + RAMPS 1.4 + DRV8825 (≈$130)**
Flash rckeith's GRBL HotWire Mega-5X, or Marlin `FOAMCUTTER_XYUV` with an LCD for standalone SD cutting.
*Pros:* the most tutorials, parts in every drawer, and the PC-free option with Marlin.
*Cons:* 8-bit, noisy drivers with Vref set by potentiometer, and a weak RAMPS power connector, so keep the wire off the D8 MOSFET.

**Upgrade path:** if you later need Ethernet, closed-loop motors, or more than 5 axes (turntable), move to a Teensy 4.1 grblHAL board (T41E5XBB) or LinuxCNC + Mesa 7i96S. The G-code and CAM stay the same.

## Sources
- rckeith GRBL HotWire Mega-5X: https://rckeith.co.uk/grbl-hotwire-mega-5x-for-cnc-foam-cutters/ · https://rckeith.co.uk/
- grblHAL SKR Pico hot-wire map: https://github.com/grblHAL/RP2040/blob/master/boards/btt_skr_pico_10_map.h
- FluidNC axes: https://github.com/bdring/FluidNC/blob/main/FluidNC/src/Machine/Axes.cpp · configs: https://github.com/bdring/fluidnc-config-files
- FluidNC foam controller: https://www.pcbway.com/project/shareproject/FluidNC_foam_cutter_controller_bad9d318.html
- Marlin config: https://github.com/MarlinFirmware/Marlin/blob/bugfix-2.1.x/Marlin/Configuration.h · https://www.github.com/DerAndere1/Marlin/issues/65
- LinuxCNC foam: https://forum.linuxcnc.org/38-general-linuxcnc-questions/26106-xyuv-foamcuttter-configuration-from-scratch-help · Mesa: https://store.mesanet.com/index.php?route=product/product&product_id=374
- Klipper GCODE_AXIS: https://github.com/Klipper3d/klipper/blob/master/docs/G-Codes.md
- grbl-xyuv: https://github.com/rasciodc/grbl-xyuv · WingGcode: https://github.com/swarfer/winggcode
- Boards: https://specpicks.com/product/B09MYKL9MP · https://www.elecrow.com/4x-cnc-controller-integrated-esp32-and-tmc2209.html · https://www.cnx-software.com/2025/01/30/fluidnc-compatible-cnc-controller-supports-6-stepper-drivers-web-based-configuration/ · https://forum.makerforums.info/t/mks-dlc32-v2-1/88240
- Mach licensing: https://www.machsupport.com/licensing/
- Hackaday 2026 build: https://hackaday.com/2026/07/12/building-a-better-cnc-hot-wire-foam-cutter/
