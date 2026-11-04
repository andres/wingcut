//! Form editors for the machine, the wing stations and the panels.
//! Each returns `true` when anything changed.

use crate::app::{App, new_panel, new_wing, relative_to};
use eframe::egui::{self, DragValue, Grid, RichText, TextEdit, Ui};
use wingcut::config::{Cutout, KerfPoint, Machine, Sheeting, Side, Surface};

fn num(ui: &mut Ui, v: &mut f64, speed: f64, suffix: &str) -> bool {
    ui.add(
        DragValue::new(v)
            .speed(speed)
            .suffix(suffix)
            .max_decimals(3),
    )
    .changed()
}

fn row(ui: &mut Ui, label: &str, hint: &str, add: impl FnOnce(&mut Ui) -> bool) -> bool {
    let r = ui.label(label);
    if !hint.is_empty() {
        r.on_hover_text(hint);
    }
    let c = add(ui);
    ui.end_row();
    c
}

pub fn machine(ui: &mut Ui, m: &mut Machine) -> bool {
    let mut ch = false;
    crate::help::heading(ui, "Machine", "machine-tab");
    Grid::new("machine")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ch |= row(ui, "Tower spacing", "Distance between the wire contacts on the two towers (mm). Used to extend the cut from the block faces out to the towers.", |ui| {
                num(ui, &mut m.tower_spacing, 1.0, " mm")
            });
            ch |= row(ui, "Axis letters", "Controller axis letters for: root-tower horizontal, root-tower vertical, tip-tower horizontal, tip-tower vertical.", |ui| {
                ui.horizontal(|ui| {
                    let mut c = false;
                    for a in &mut m.axes {
                        c |= ui
                            .add(TextEdit::singleline(a).desired_width(24.0))
                            .changed();
                    }
                    c
                })
                .inner
            });
            ch |= row(ui, "X limits", "Front–back travel of both towers (min, max mm). Every point of the cut is checked against it.", |ui| {
                ui.horizontal(|ui| {
                    num(ui, &mut m.limits.x[0], 1.0, " mm")
                        | num(ui, &mut m.limits.x[1], 1.0, " mm")
                })
                .inner
            });
            ch |= row(ui, "Y limits", "Up–down travel of both towers (min, max mm).", |ui| {
                ui.horizontal(|ui| {
                    num(ui, &mut m.limits.y[0], 1.0, " mm")
                        | num(ui, &mut m.limits.y[1], 1.0, " mm")
                })
                .inner
            });
            ch |= row(ui, "Cut feed", "Cutting speed (mm/min) of the faster block face, usually the panel's inner (larger) end. The other face moves slower in proportion. Typical 120–240.", |ui| {
                num(ui, &mut m.feed_cut, 1.0, " mm/min")
            });
            ch |= row(ui, "Rapid feed", "Speed of moves with the wire cold: into position and parking (mm/min).", |ui| {
                num(ui, &mut m.feed_rapid, 5.0, " mm/min")
            });
            ch |= row(ui, "Heat on", "Command that switches the wire on. FluidNC: M3 S1000. The heat itself is set on the supply (amps).", |ui| {
                ui.text_edit_singleline(&mut m.heat_on).changed()
            });
            ch |= row(ui, "Heat off", "Command that switches the wire off (M5).", |ui| {
                ui.text_edit_singleline(&mut m.heat_off).changed()
            });
            ch |= row(ui, "Preheat", "Wait after switching the wire on, before entering the foam (s).", |ui| {
                num(ui, &mut m.preheat_s, 0.1, " s")
            });
            ch |= row(ui, "LE dwell", "Optional pause at the leading edge (s); 0 = none.", |ui| {
                num(ui, &mut m.le_dwell_s, 0.1, " s")
            });
            ch |= row(
                ui,
                "TE min thickness",
                "The thinnest the trailing edge may be (mm). Thinner foam melts away or snaps; 0.8 is a good start.",
                |ui| num(ui, &mut m.te_min_thickness, 0.05, " mm"),
            );
            ch |= row(
                ui,
                "Max segment",
                "Longest straight step along the profile (mm). Smaller = smoother but longer G-code; 0.2 is plenty.",
                |ui| num(ui, &mut m.max_segment, 0.01, " mm"),
            );
            ch |= row(
                ui,
                "Lead clearance",
                "How far in front of the block the wire starts and ends (mm).",
                |ui| num(ui, &mut m.lead_clearance, 0.5, " mm"),
            );
            ch |= row(ui, "Park X", "Where both towers go front–back after the cut (mm); the wire stays at its height.", |ui| {
                num(ui, &mut m.park_x, 1.0, " mm")
            });
            ch |= row(
                ui,
                "Miter limit",
                "How far kerf compensation may reach out at sharp corners, as a multiple of half the kerf. Rarely needs changing.",
                |ui| num(ui, &mut m.miter_limit, 0.1, ""),
            );
        });
    ui.add_space(8.0);
    crate::help::label(
        ui,
        "Kerf vs face speed",
        "kerf-table",
        "From your test cuts; interpolated, clamped at the ends",
    );
    let mut remove = None;
    Grid::new("kerf")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.label("feed");
            ui.label("kerf");
            ui.end_row();
            for (i, k) in m.kerf.iter_mut().enumerate() {
                ch |= num(ui, &mut k.feed, 1.0, " mm/min");
                ch |= num(ui, &mut k.kerf, 0.01, " mm");
                if ui.small_button("✖").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        m.kerf.remove(i);
        ch = true;
    }
    if ui.button("+ kerf row").clicked() {
        let last = m.kerf.last().copied().unwrap_or(KerfPoint {
            feed: 180.0,
            kerf: 0.8,
        });
        m.kerf.push(KerfPoint {
            feed: last.feed + 60.0,
            kerf: last.kerf,
        });
        ch = true;
    }
    ch
}

pub fn wing(ui: &mut Ui, app: &mut App) -> bool {
    let mut ch = false;
    ui.horizontal(|ui| {
        ui.label("Wing name")
            .on_hover_text("Written into the G-code header");
        ch |= ui.text_edit_singleline(&mut app.wing.name).changed();
        crate::help::button(ui, "wing-tab");
    });
    ui.add_space(6.0);
    egui::CollapsingHeader::new(RichText::new("Stations").heading())
        .default_open(true)
        .show(ui, |ui| {
            crate::help::label(
                ui,
                "The wing's shape at chosen points along one half-span",
                "stations",
                "Everything between two stations is a straight blend of the two",
            );
            ch |= stations(ui, app);
        });
    ui.add_space(6.0);
    egui::CollapsingHeader::new(RichText::new("Panels").heading())
        .default_open(true)
        .show(ui, |ui| {
            crate::help::label(ui, "One panel = one foam block = one cut", "panels", "");
            ch |= panels(ui, app);
        });
    ch
}

fn stations(ui: &mut Ui, app: &mut App) -> bool {
    let mut ch = false;
    let mut remove = None;
    let base = app.wing.base_dir.clone();
    Grid::new("stations")
        .num_columns(8)
        .striped(true)
        .show(ui, |ui| {
            for (h, tip) in [
                ("span mm", "Distance from the wing centreline (0) outwards"),
                (
                    "chord mm",
                    "Airfoil length from leading edge to trailing edge",
                ),
                (
                    "LE offset mm",
                    "Sweep: how far back the leading edge sits (+ = aft, − = forward)",
                ),
                (
                    "dihedral mm",
                    "How far up the section sits (+ = up, − = down)",
                ),
                ("twist °", "+ = nose up, − = nose down (washout)"),
                (
                    "pivot",
                    "Twist pivot as a fraction of the chord (0.25 = quarter chord)",
                ),
                ("airfoil", "A .dat (Selig/Lednicer) or .dxf profile file"),
                ("", ""),
            ] {
                let r = ui.label(RichText::new(h).small().strong());
                if !tip.is_empty() {
                    r.on_hover_text(tip);
                }
            }
            ui.end_row();
            for (i, s) in app.wing.stations.iter_mut().enumerate() {
                ch |= num(ui, &mut s.span, 1.0, "");
                ch |= num(ui, &mut s.chord, 0.5, "");
                ch |= num(ui, &mut s.le_offset, 0.5, "");
                ch |= num(ui, &mut s.dihedral_offset, 0.5, "");
                ch |= ui
                    .add(
                        DragValue::new(&mut s.twist_deg)
                            .speed(0.05)
                            .suffix("°")
                            .max_decimals(2),
                    )
                    .changed();
                ch |= ui
                    .add(
                        DragValue::new(&mut s.twist_pivot)
                            .speed(0.01)
                            .range(0.0..=1.0)
                            .max_decimals(2),
                    )
                    .changed();
                ui.horizontal(|ui| {
                    let mut text = s.airfoil.to_string_lossy().into_owned();
                    let r = ui.add(
                        TextEdit::singleline(&mut text)
                            .desired_width(120.0)
                            .hint_text("choose…"),
                    );
                    if r.changed() {
                        s.airfoil = text.into();
                        ch = true;
                    }
                    if ui
                        .small_button("…")
                        .on_hover_text("Choose a .dat or .dxf profile")
                        .clicked()
                        && let Some(p) = rfd::FileDialog::new()
                            .add_filter("Airfoil", &["dat", "dxf", "DAT", "DXF"])
                            .set_directory(&base)
                            .pick_file()
                    {
                        s.airfoil = relative_to(&p, &base);
                        ch = true;
                    }
                });
                if ui
                    .small_button("✖")
                    .on_hover_text("Remove station")
                    .clicked()
                {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    ui.label(
        RichText::new("Hover a column title for help. Stations must be in order of span; the first is usually 0 (the centreline).")
            .small()
            .weak(),
    );
    if let Some(i) = remove {
        app.wing.stations.remove(i);
        ch = true;
    }
    if ui.button("+ station").clicked() {
        let mut s = app
            .wing
            .stations
            .last()
            .cloned()
            .unwrap_or_else(|| new_wing().stations[0].clone());
        s.span += 500.0;
        app.wing.stations.push(s);
        ch = true;
    }
    ch
}

fn panels(ui: &mut Ui, app: &mut App) -> bool {
    let mut ch = false;
    let mut selected_changed = false;
    let spacing = app.machine.tower_spacing;
    // The selected panel is also planned: show its problems too.
    let planned: Vec<String> = match &app.preview {
        Some(Ok(p)) if !p.violations.is_empty() => {
            vec![format!(
                "{} points outside the axis limits",
                p.violations.len()
            )]
        }
        Some(Err(e)) => vec![e.clone()],
        _ => Vec::new(),
    };
    Grid::new("panel-list")
        .num_columns(5)
        .striped(true)
        .show(ui, |ui| {
            for (h, tip) in [
                (
                    "panel",
                    "Click to edit; the preview shows the selected panel",
                ),
                (
                    "span mm",
                    "Where the panel sits along the half-span (from → to)",
                ),
                ("length", "Panel length = foam block length"),
                ("side", "Left or right wing half"),
                ("", ""),
            ] {
                let r = ui.label(RichText::new(h).small().strong());
                if !tip.is_empty() {
                    r.on_hover_text(tip);
                }
            }
            ui.end_row();
            for (i, p) in app.wing.panels.iter().enumerate() {
                if ui.selectable_label(app.selected == i, &p.name).clicked() && app.selected != i {
                    app.selected = i;
                    selected_changed = true;
                }
                ui.label(format!("{:.0} → {:.0}", p.from, p.to));
                ui.label(format!("{:.0}", p.to - p.from));
                ui.label(if p.side == Side::Left {
                    "left"
                } else {
                    "right"
                });
                let mut issues = wingcut::layout::panel_issues(&app.wing, p, spacing);
                if i == app.selected {
                    issues.extend(planned.iter().cloned());
                }
                if issues.is_empty() {
                    ui.label(RichText::new("✔").color(egui::Color32::from_rgb(60, 170, 90)));
                } else {
                    ui.label(RichText::new("⚠").color(egui::Color32::from_rgb(220, 170, 30)))
                        .on_hover_text(issues.join("\n"));
                }
                ui.end_row();
            }
        });
    if ui
        .button("Split into panels…")
        .on_hover_text("Create the panels for the whole wing automatically, sized for your machine")
        .clicked()
    {
        app.split = Some(SplitDialog::new(&app.machine));
    }
    ui.horizontal(|ui| {
        if ui.button("+ new").clicked() {
            let (from, to) = app
                .wing
                .stations
                .first()
                .zip(app.wing.stations.last())
                .map_or((0.0, 1000.0), |(a, b)| (a.span, b.span));
            let name = format!("panel-{}", app.wing.panels.len() + 1);
            app.wing.panels.push(new_panel(&name, from, to));
            app.selected = app.wing.panels.len() - 1;
            ch = true;
        }
        if let Some(p) = app.wing.panels.get(app.selected).cloned() {
            if ui.button("duplicate").clicked() {
                let mut q = p.clone();
                q.name = format!("{}-copy", p.name);
                app.wing.panels.push(q);
                app.selected = app.wing.panels.len() - 1;
                ch = true;
            }
            if ui
                .button("mirror")
                .on_hover_text("Duplicate as the other wing side")
                .clicked()
            {
                let mut q = p.clone();
                q.side = if p.side == Side::Left {
                    Side::Right
                } else {
                    Side::Left
                };
                q.name = mirrored_name(&p.name);
                app.wing.panels.push(q);
                app.selected = app.wing.panels.len() - 1;
                ch = true;
            }
            if ui.button("delete").clicked() {
                app.wing.panels.remove(app.selected);
                app.selected = app.selected.saturating_sub(1);
                ch = true;
            }
        }
    });
    ui.separator();
    if let Some(p) = app.wing.panels.get_mut(app.selected) {
        crate::help::label(
            ui,
            &format!("Panel {}", p.name),
            "panel-and-block-settings",
            "",
        );
        Grid::new("panel").num_columns(2).striped(true).show(ui, |ui| {
            ch |= row(ui, "Name", "Used for the output file names", |ui| ui.text_edit_singleline(&mut p.name).changed());
            ch |= row(ui, "Span from / to", "Where the panel starts and ends along the half-span (mm from the centreline). The panel's length is the foam block's length.", |ui| {
                ui.horizontal(|ui| num(ui, &mut p.from, 1.0, " mm") | num(ui, &mut p.to, 1.0, " mm")).inner
            });
            ch |= row(ui, "Side", "Which wing half. Left: the panel's inner end (towards the fuselage) is on the root-tower face of the block. Right: mirrored, inner end on the tip-tower face.", |ui| {
                ui.horizontal(|ui| ui.selectable_value(&mut p.side, Side::Left, "left").changed() | ui.selectable_value(&mut p.side, Side::Right, "right").changed()).inner
            });
            let b = &mut p.block;
            ch |= row(ui, "Block position s", "Where the block sits between the towers: distance from the root tower's wire contact to the nearest block face (mm). Use 'centre between towers' below.", |ui| num(ui, &mut b.s_root, 1.0, " mm"));
            ch |= row(ui, "Block front x0", "Machine x (front–back) of the block's front face, the side facing you. The wire starts in front of it and enters here.", |ui| num(ui, &mut b.x0, 1.0, " mm"));
            ch |= row(ui, "Block bottom y0", "Machine height of the block's underside (what it sits on), mm.", |ui| num(ui, &mut b.y0, 1.0, " mm"));
            ch |= row(ui, "Block height / depth", "Block height (y) and depth front-to-back (x), mm. The profile must fit inside, with room for the skins and slots.", |ui| {
                ui.horizontal(|ui| num(ui, &mut b.height, 1.0, " mm") | num(ui, &mut b.width, 1.0, " mm")).inner
            });
            ch |= row(ui, "TE inset", "Gap between the block's front face and the trailing edge that is closest to it (mm). The trailing edge faces you; the leading edge points away.", |ui| num(ui, &mut b.inset, 0.5, " mm"));
            ch |= row(ui, "Profile height", "Height of the wing's reference line (y = 0) above the block bottom. Auto centres the profiles vertically in the block.", |ui| {
                ui.horizontal(|ui| {
                    let mut auto = b.chord_y.is_none();
                    let mut c = ui.checkbox(&mut auto, "auto").changed();
                    if c {
                        b.chord_y = if auto { None } else { Some(b.height / 2.0) };
                    }
                    if let Some(v) = b.chord_y.as_mut() {
                        c |= num(ui, v, 0.5, " mm");
                    }
                    c
                })
                .inner
            });
        });
        let len = p.to - p.from;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "Block {len:.0} mm long, faces at s = {:.0} and {:.0}: {:.0} mm from the root tower, {:.0} mm from the tip tower.",
                    p.block.s_root,
                    p.block.s_root + len,
                    p.block.s_root,
                    spacing - p.block.s_root - len
                ))
                .small(),
            );
            if ui.small_button("centre between towers").clicked() {
                p.block.s_root = ((spacing - len) / 2.0).max(0.0);
                ch = true;
            }
        });
        ui.add_space(6.0);
        ch |= sheeting(ui, &mut p.sheeting);
        ui.add_space(6.0);
        ch |= cutouts(ui, &mut p.cutouts);
    } else {
        ui.label("No panels yet.");
    }
    if selected_changed {
        app.dirty = true; // re-plan, but the file itself is unchanged
    }
    ch
}

/// The "Split into panels" dialog state.
pub struct SplitDialog {
    pub max_len: f64,
    pub at_stations: bool,
    pub count: usize,
    pub sides: wingcut::layout::Sides,
}

impl SplitDialog {
    pub fn new(m: &Machine) -> SplitDialog {
        SplitDialog {
            // Leave at least 50 mm between the block and each tower.
            max_len: (m.tower_spacing - 100.0).clamp(100.0, 1250.0),
            at_stations: true,
            count: 2,
            sides: wingcut::layout::Sides::Both,
        }
    }
}

/// The split dialog; replaces the wing's panels on confirmation.
pub fn split_dialog(ctx: &egui::Context, app: &mut App) {
    use wingcut::layout::{Sides, Split, intervals, make_panels};
    let Some(d) = app.split.as_mut() else { return };
    let mut apply = false;
    let mut close = false;
    let split = if d.at_stations {
        Split::AtStations
    } else {
        Split::Equal(d.count)
    };
    let iv = intervals(&app.wing, split, d.max_len);
    egui::Modal::new(egui::Id::unique("split")).show(ctx, |ui| {
        ui.set_max_width(520.0);
        crate::help::heading(ui, "Split the wing into panels", "split-into-panels");
        ui.label("Each panel is cut from one foam block, so it can't be longer than your machine allows. This creates every panel for the whole wing and centres each block between the towers.");
        ui.add_space(4.0);
        Grid::new("split").num_columns(2).show(ui, |ui| {
            ui.label("Longest panel").on_hover_text("The longest block you can cut: less than the tower spacing, and no longer than your foam");
            ui.add(DragValue::new(&mut d.max_len).speed(5.0).range(50.0..=5000.0).suffix(" mm"));
            ui.end_row();
            ui.label("Split");
            ui.vertical(|ui| {
                ui.radio_value(&mut d.at_stations, true, "at the stations (recommended)")
                    .on_hover_text("No station falls inside a panel, so every kink in the planform is cut. Long intervals are divided equally.");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.at_stations, false, "into equal panels:");
                    ui.add(DragValue::new(&mut d.count).range(1..=20));
                    ui.label("per side");
                });
            });
            ui.end_row();
            ui.label("Wing halves");
            ui.horizontal(|ui| {
                ui.radio_value(&mut d.sides, Sides::Both, "both");
                ui.radio_value(&mut d.sides, Sides::Left, "left only");
                ui.radio_value(&mut d.sides, Sides::Right, "right only");
            });
            ui.end_row();
        });
        ui.add_space(4.0);
        let lens: Vec<String> = iv.iter().map(|(a, b)| format!("{:.0}–{:.0} ({:.0} mm)", a, b, b - a)).collect();
        ui.label(RichText::new(format!("Panels per side: {}", lens.join(", "))).small());
        let per = if d.sides == Sides::Both { 2 } else { 1 };
        ui.label(
            RichText::new("Block size and sheeting are copied from the selected panel; cutouts (spar notches, hinges) are not, so add them where you need them.")
                .small()
                .weak(),
        );
        ui.horizontal(|ui| {
            let text = format!("Create {} panels (replaces the {} you have)", iv.len() * per, app.wing.panels.len());
            if ui.add_enabled(!iv.is_empty(), egui::Button::new(text)).clicked() {
                apply = true;
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });
    if apply {
        let template = app
            .wing
            .panels
            .get(app.selected)
            .cloned()
            .unwrap_or_else(|| new_panel("t", 0.0, 1.0));
        let sides = d.sides;
        app.wing.panels = make_panels(&iv, sides, &template, app.machine.tower_spacing);
        app.selected = 0;
        app.wing_unsaved = true;
        app.dirty = true;
        app.split = None;
    } else if close {
        app.split = None;
    }
}

fn sheeting(ui: &mut Ui, sh: &mut Sheeting) -> bool {
    let mut ch = false;
    crate::help::label(
        ui,
        "Sheeting",
        "sheeting",
        "Skin glued on the core. The core is cut smaller by these thicknesses so the sheeted wing has the true airfoil. 0 = no skin.",
    );
    Grid::new("sheeting").num_columns(2).striped(true).show(ui, |ui| {
        ch |= row(ui, "Top / bottom skin", "Skin thickness on the upper and lower surface, behind the front zone", |ui| {
            ui.horizontal(|ui| num(ui, &mut sh.top, 0.05, " mm") | num(ui, &mut sh.bottom, 0.05, " mm")).inner
        });
        ch |= row(ui, "Front zone skin", "Skin over the front zone, top and bottom and around the nose (e.g. a thicker D-box). 0 = none.", |ui| {
            ui.horizontal(|ui| {
                let c = num(ui, &mut sh.front, 0.05, " mm");
                ui.label("up to");
                c | ui
                    .add(DragValue::new(&mut sh.front_to).speed(0.005).range(0.0..=1.0).max_decimals(3))
                    .on_hover_text("Chord fraction where the front zone ends (0.30 = 30 %)")
                    .changed()
            })
            .inner
        });
        ch |= row(ui, "LE block", "Solid balsa leading edge: the core's nose is cut off flat at this chord fraction, and a balsa block is glued on and sanded to shape.", |ui| {
            fraction_toggle(ui, &mut sh.le_cap, 0.0, 0.03, "none (skin wraps the nose)")
        });
        ch |= row(ui, "TE stock", "Balsa trailing edge: the core is cut off flat at this chord fraction and a balsa TE strip is glued on.", |ui| {
            fraction_toggle(ui, &mut sh.te_stock, 1.0, 0.9, "none (core runs to the TE)")
        });
    });
    ch
}

/// A chord fraction that can be switched off (`off` value) or set.
fn fraction_toggle(ui: &mut Ui, v: &mut f64, off: f64, default_on: f64, off_text: &str) -> bool {
    ui.horizontal(|ui| {
        let mut on = *v != off;
        let text = if on { "at" } else { off_text };
        let mut c = ui.checkbox(&mut on, text).changed();
        if c {
            *v = if on { default_on } else { off };
        }
        if on {
            c |= ui
                .add(
                    DragValue::new(v)
                        .speed(0.002)
                        .range(0.0..=1.0)
                        .max_decimals(3)
                        .suffix(" of chord"),
                )
                .changed();
        }
        c
    })
    .inner
}

fn cutouts(ui: &mut Ui, cuts: &mut Vec<Cutout>) -> bool {
    let mut ch = false;
    crate::help::label(
        ui,
        "Cutouts",
        "cutouts",
        "Slots and V-grooves cut into the core. Same chord fraction on both faces; sizes in mm. A hinge cut comes in from one surface and leaves a thin skin at the other, where the hinge is.",
    );
    let mut remove = None;
    Grid::new("cutouts")
        .num_columns(7)
        .striped(true)
        .show(ui, |ui| {
            for h in ["cut from", "x (chord)", "shape", "", "size", "", ""] {
                ui.label(RichText::new(h).small().strong());
            }
            ui.end_row();
            for (i, c) in cuts.iter_mut().enumerate() {
                ch |= ui
                    .horizontal(|ui| {
                        ui.selectable_value(&mut c.surface, Surface::Upper, "upper")
                            .on_hover_text(
                                "Cut from the upper surface (a hinge is then on the bottom)",
                            )
                            .changed()
                            | ui.selectable_value(&mut c.surface, Surface::Lower, "lower")
                                .on_hover_text(
                                    "Cut from the lower surface (a hinge is then on top)",
                                )
                                .changed()
                    })
                    .inner;
                ch |= ui
                    .add(
                        DragValue::new(&mut c.x)
                            .speed(0.002)
                            .range(0.0..=1.0)
                            .max_decimals(3),
                    )
                    .changed();
                // Shape: slot (width) or V-groove (angle).
                let mut v = c.angle.is_some();
                let shape_changed = ui
                    .horizontal(|ui| {
                        ui.selectable_value(&mut v, false, "slot")
                            .on_hover_text("Rectangular slot")
                            .changed()
                            | ui.selectable_value(&mut v, true, "V")
                                .on_hover_text("V-groove: lets an aileron deflect")
                                .changed()
                    })
                    .inner;
                if shape_changed {
                    (c.width, c.angle) = if v {
                        (None, Some(30.0))
                    } else {
                        (Some(1.5), None)
                    };
                    ch = true;
                }
                match (c.width.as_mut(), c.angle.as_mut()) {
                    (Some(w), _) => {
                        ch |= ui
                            .add(
                                DragValue::new(w)
                                    .speed(0.1)
                                    .suffix(" mm wide")
                                    .max_decimals(2),
                            )
                            .changed()
                    }
                    (None, Some(a)) => {
                        ch |= ui
                            .add(
                                DragValue::new(a)
                                    .speed(0.5)
                                    .range(1.0..=170.0)
                                    .suffix("° open")
                                    .max_decimals(1),
                            )
                            .on_hover_text("Total opening angle; about twice the aileron throw")
                            .changed()
                    }
                    _ => {
                        ui.label("");
                    }
                }
                // Depth from the cut surface, or skin left at the other surface.
                let mut spar = c.depth.is_some();
                let kind_changed = ui
                    .horizontal(|ui| {
                        ui.selectable_value(&mut spar, true, "depth")
                            .on_hover_text("Spar notch: depth below the cut surface")
                            .changed()
                            | ui.selectable_value(&mut spar, false, "leave")
                                .on_hover_text("Hinge: material kept at the other surface")
                                .changed()
                    })
                    .inner;
                if kind_changed {
                    (c.depth, c.leave) = if spar {
                        (Some(2.0), None)
                    } else {
                        (None, Some(0.5))
                    };
                    ch = true;
                }
                if let Some(d) = c.depth.as_mut().or(c.leave.as_mut()) {
                    ch |= num(ui, d, 0.05, " mm");
                } else {
                    ui.label("");
                }
                if ui.small_button("✖").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        cuts.remove(i);
        ch = true;
    }
    ui.horizontal(|ui| {
        let mut add = |c: Cutout| {
            cuts.push(c);
            ch = true;
        };
        let base = Cutout {
            surface: Surface::Upper,
            x: 0.3,
            width: Some(10.0),
            angle: None,
            depth: Some(2.0),
            leave: None,
        };
        if ui.button("+ spar notch").clicked() {
            add(base.clone());
        }
        if ui.button("+ hinge slot").clicked() {
            add(Cutout {
                surface: Surface::Lower,
                x: 0.75,
                width: Some(1.5),
                depth: None,
                leave: Some(0.5),
                ..base.clone()
            });
        }
        if ui.button("+ hinge V").clicked() {
            add(Cutout {
                surface: Surface::Lower,
                x: 0.75,
                width: None,
                angle: Some(30.0),
                depth: None,
                leave: Some(0.5),
            });
        }
    });
    ch
}

fn mirrored_name(name: &str) -> String {
    if let Some(stem) = name.strip_suffix("-L") {
        format!("{stem}-R")
    } else if let Some(stem) = name.strip_suffix("-R") {
        format!("{stem}-L")
    } else {
        format!("{name}-mirror")
    }
}

#[cfg(test)]
mod tests {
    use super::mirrored_name;

    #[test]
    fn mirror_names_swap_the_side_suffix() {
        assert_eq!(mirrored_name("center-L"), "center-R");
        assert_eq!(mirrored_name("tip-R"), "tip-L");
        assert_eq!(mirrored_name("wing"), "wing-mirror");
    }
}
