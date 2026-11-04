//! Links into the user guide (docs/help.md on GitHub).

use eframe::egui::{self, RichText, Ui};

pub const GUIDE: &str = "https://github.com/andres/wingcut/blob/main/docs/help.md";
pub const BRINGUP: &str = "https://github.com/andres/wingcut/blob/main/docs/hardware-bringup.md";

/// Open the guide at a section (`anchor` = GitHub heading anchor).
pub fn open(ctx: &egui::Context, anchor: &str) {
    let url = if anchor.is_empty() {
        GUIDE.to_string()
    } else {
        format!("{GUIDE}#{anchor}")
    };
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

/// A small "?" button that opens the guide at `anchor`.
pub fn button(ui: &mut Ui, anchor: &str) {
    if ui
        .small_button("?")
        .on_hover_text("Open the user guide for this section")
        .clicked()
    {
        open(ui.ctx(), anchor);
    }
}

/// A section heading with a "?" link.
pub fn heading(ui: &mut Ui, text: &str, anchor: &str) {
    ui.horizontal(|ui| {
        ui.heading(text);
        button(ui, anchor);
    });
}

/// A bold sub-heading with a "?" link.
pub fn label(ui: &mut Ui, text: &str, anchor: &str, hover: &str) {
    ui.horizontal(|ui| {
        let r = ui.label(RichText::new(text).strong());
        if !hover.is_empty() {
            r.on_hover_text(hover);
        }
        button(ui, anchor);
    });
}

/// The Help menu.
pub fn menu(ui: &mut Ui) {
    ui.menu_button("Help", |ui| {
        for (text, anchor) in [
            ("User guide", ""),
            ("Quick start: your first wing", "quick-start"),
            ("Example: a 5 m wing", "worked-example-a-5-m-wing"),
            ("Signs and directions (+ / −)", "signs-and-directions"),
            ("Warnings and errors", "warnings-and-errors"),
        ] {
            if ui.button(text).clicked() {
                open(ui.ctx(), anchor);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Hardware bring-up checklist").clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(BRINGUP));
            ui.close();
        }
        ui.separator();
        ui.label(
            RichText::new(format!(
                "wingcut {} · MIT licence",
                env!("CARGO_PKG_VERSION")
            ))
            .weak(),
        );
    });
}
