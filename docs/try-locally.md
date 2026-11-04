# Trying wingcut on your Linux desktop

```bash
git clone https://github.com/andres/wingcut
cd wingcut
bash scripts/try-local.sh
```

The script installs nothing without asking. It checks for Rust (and offers to install it with rustup into `~/.cargo`), runs all tests, builds the release binaries, generates the example 4 m wing into `./out`, and opens the GUI. Everything it prints is saved to `try-local.log`.

Needs: a C linker (`build-essential` on Debian/Ubuntu) and a normal desktop (X11 or Wayland). The first build takes a few minutes.

## What to check in the GUI

1. **Preview**: `center-L` shows two faces (blue = wing root, red = wing tip) with spar slots, green/orange tower paths, and the yellow block.
2. **Live edit**: drag the root chord (Stations, first row) and watch the preview follow. The title gets a `*`.
3. **Panels**: click `tip-L` to see the hinge cut. Open the **Planform** tab to see the whole wing.
4. **Limits**: set *Block bottom y0* to `-15`. Red points appear, the bottom bar explains the problem, and **Generate all** refuses to write.
5. **Generate**: undo that (y0 = `20`) and press **Generate all**. The files land in `examples/out/`. Open an `.svg` in a browser.
6. **Save**: File > Save wing as... to a new file (keep the example clean). Close and reopen it.
7. **Quit guard**: edit something, then File > Quit. You should get *Save changes?*.
8. **DXF**: on a station, `…` > `examples/airfoils/naca2410.dxf`. The preview should look the same as with the `.dat`.

## Using the binaries directly

```bash
./target/release/wingcut --help
./target/release/wingcut-gui path/to/wing.toml
```

Copy the two files from `target/release/` anywhere on your `PATH` (e.g. `~/.local/bin`) to use them like normal programs.
