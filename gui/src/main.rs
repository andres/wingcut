//! wingcut-gui: edit machine and wing configs with a live cut preview.
//!
//! Usage: `wingcut-gui [--machine machine.toml] [wing.toml]`

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")] // no console window

mod app;
mod editors;
mod preview;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let mut machine = None;
    let mut wing = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        if a == "--machine" {
            machine = args.next().map(PathBuf::from);
        } else if a == "--help" || a == "-h" {
            println!("usage: wingcut-gui [--machine machine.toml] [wing.toml]");
            return Ok(());
        } else {
            wing = Some(PathBuf::from(a));
        }
    }
    let options = |renderer| eframe::NativeOptions {
        renderer,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 820.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("wingcut"),
        ..Default::default()
    };
    let run = |renderer| {
        let (machine, wing) = (machine.clone(), wing.clone());
        eframe::run_native(
            "wingcut",
            options(renderer),
            Box::new(move |_cc| Ok(Box::new(app::App::new(machine, wing)))),
        )
    };
    // wgpu (Vulkan/Metal/DX12) first; plain OpenGL for old PCs and VMs.
    run(eframe::Renderer::Wgpu).or_else(|e| {
        eprintln!("wgpu renderer failed ({e}); falling back to OpenGL");
        run(eframe::Renderer::Glow)
    })
}
