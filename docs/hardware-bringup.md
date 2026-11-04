# Hardware bring-up checklist (stage 4)

Machine control was built and tested against a **simulated** FluidNC board
and a **simulated** Riden supply. The points below could only be checked on
real hardware. Work through them on the bench (hardware plan §6 step 2),
**motors connected but no wire tension and no foam**, before the first cut.
Note anything that differs and send the log (Control tab console) back.

## Early test without the real board (optional, ~$10)

A bare ESP32 dev board running FluidNC (web installer, any config) is enough
to test the connection, status and streaming before the 4x board arrives.
Use **dry run**.

## 1. Connection

- [ ] USB: the port shows in the Control tab list (Linux: `/dev/ttyUSB0` or
      `/dev/ttyACM0`). If it won't open: `sudo usermod -aG dialout $USER`,
      then log out and in.
- [ ] Connecting does **not** reboot the board (wingcut keeps DTR/RTS low).
      If the banner reappears on every connect, note it.
- [ ] Wi-Fi telnet (port 23) and WebSocket (port 81): both connect and show
      the status. WebSocket input to FluidNC is the least certain part; if
      commands don't get answers over WebSocket, use telnet or USB.

## 2. Status and position

- [ ] The state (Idle/Alarm) and the four positions update about 5×/s.
- [ ] Jog `both towers` +10 mm moves X and Z together; `root only` moves X
      only. Check the directions: **to back ▶ = away from you (+x)**,
      **▲ up = up (+y)**. If an axis is reversed, fix it in FluidNC's
      `config.yaml` (`direction_pin` invert), not in wingcut.
- [ ] The tower positions shown match where the wire is (the pink diamond
      on the preview). With axes other than X Y Z A, check the order.
- [ ] Home works; the "not homed" warning clears.

## 3. Wire switching (most important)

With the wire **disconnected** (watch the MOSFET LED or measure):

- [ ] `M3 S1000` typed in the console switches the wire on; `M5` off. The
      panel shows **wire ON** / **wire off**. (It reads the spindle value
      from FluidNC's `FS:` field.)
- [ ] Run a panel, press **Pause** during the cut: motion stops **and the
      wire switches off** (wingcut sends FluidNC's spindle-stop override,
      `0x9E`, once the hold completes). **Resume** switches it back on and
      continues. If the wire stays on during a pause, stop and report:
      FluidNC's spindle-stop behaviour may need a config change.
- [ ] **STOP** during a cut: motion stops, the wire switches off, no alarm.
- [ ] Unplug USB during a dry run: wingcut reports the lost connection.
      Remember the board finishes its buffered moves; **the E-stop is the
      safety**, not the PC.
- [ ] The E-stop kills both motion and wire current.

## 4. A full dry run

- [ ] Park the wire in front of the block position, home, run `center-L`
      with **dry run** ticked. The pre-flight checks pass; the machine
      traces the cut with the wire cold; the time matches the estimate
      roughly; it parks at the end.

## 5. Riden RD6006 (if used)

- [ ] On the Riden: set the communication interface to USB, note the baud
      rate and address (menu). Enter them in Wire supply.
- [ ] **Connect supply**: the model shows (e.g. RD6006) and the live V/A
      readout appears. If it times out, check port, baud and address.
- [ ] **Apply current** changes the set current on the supply's display
      (scales are per model; RD6006 = 1 mA steps). If the value is off by
      10× or 100×, report the model number.
- [ ] Start a real cut on a scrap block: the current is set, the output
      switches on, V/A/Ω show live (0.4 mm NiCr at 1.7 m ≈ 15 Ω cold).
- [ ] Broken-wire check: lift one wire clamp (or switch the MOSFET supply
      off) during a cut on scrap: within about a second the cut pauses, the
      wire and the supply output switch off. Tune the threshold if needed.
- [ ] After the cut (finished or stopped) the supply output is off.
