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
    ui.heading("Machine");
    Grid::new("machine")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ch |= row(ui, "Tower spacing", "Wire contact to wire contact", |ui| {
                num(ui, &mut m.tower_spacing, 1.0, " mm")
            });
            ch |= row(ui, "Axis letters", "Root x, root y, tip x, tip y", |ui| {
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
            ch |= row(ui, "X limits", "Horizontal travel (both towers)", |ui| {
                ui.horizontal(|ui| {
                    num(ui, &mut m.limits.x[0], 1.0, " mm")
                        | num(ui, &mut m.limits.x[1], 1.0, " mm")
                })
                .inner
            });
            ch |= row(ui, "Y limits", "Vertical travel (both towers)", |ui| {
                ui.horizontal(|ui| {
                    num(ui, &mut m.limits.y[0], 1.0, " mm")
                        | num(ui, &mut m.limits.y[1], 1.0, " mm")
                })
                .inner
            });
            ch |= row(ui, "Cut feed", "Speed of the faster block face", |ui| {
                num(ui, &mut m.feed_cut, 1.0, " mm/min")
            });
            ch |= row(ui, "Rapid feed", "Wire-off moves", |ui| {
                num(ui, &mut m.feed_rapid, 5.0, " mm/min")
            });
            ch |= row(ui, "Heat on", "G-code that switches the wire on", |ui| {
                ui.text_edit_singleline(&mut m.heat_on).changed()
            });
            ch |= row(ui, "Heat off", "", |ui| {
                ui.text_edit_singleline(&mut m.heat_off).changed()
            });
            ch |= row(ui, "Preheat", "Dwell after wire on", |ui| {
                num(ui, &mut m.preheat_s, 0.1, " s")
            });
            ch |= row(ui, "LE dwell", "Optional pause at the leading edge", |ui| {
                num(ui, &mut m.le_dwell_s, 0.1, " s")
            });
            ch |= row(
                ui,
                "TE min thickness",
                "So the trailing edge doesn't burn away",
                |ui| num(ui, &mut m.te_min_thickness, 0.05, " mm"),
            );
            ch |= row(
                ui,
                "Max segment",
                "Profile resolution at the largest chord",
                |ui| num(ui, &mut m.max_segment, 0.01, " mm"),
            );
            ch |= row(
                ui,
                "Lead clearance",
                "Wire start/end in front of the block",
                |ui| num(ui, &mut m.lead_clearance, 0.5, " mm"),
            );
            ch |= row(ui, "Park X", "Tower x after the cut", |ui| {
                num(ui, &mut m.park_x, 1.0, " mm")
            });
            ch |= row(
                ui,
                "Miter limit",
                "Kerf offset at sharp corners, × kerf/2",
                |ui| num(ui, &mut m.miter_limit, 0.1, ""),
            );
        });
    ui.add_space(8.0);
    ui.label(RichText::new("Kerf vs face speed").strong())
        .on_hover_text("From your test cuts; interpolated, clamped at the ends");
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
        ui.label("Wing name");
        ch |= ui.text_edit_singleline(&mut app.wing.name).changed();
    });
    ui.add_space(6.0);
    egui::CollapsingHeader::new(RichText::new("Stations").heading())
        .default_open(true)
        .show(ui, |ui| {
            ch |= stations(ui, app);
        });
    ui.add_space(6.0);
    egui::CollapsingHeader::new(RichText::new("Panels").heading())
        .default_open(true)
        .show(ui, |ui| {
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
            for h in [
                "span",
                "chord",
                "LE offset",
                "dihedral",
                "twist",
                "pivot",
                "airfoil",
                "",
            ] {
                ui.label(RichText::new(h).small().strong());
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
        RichText::new("mm, deg; twist: + nose up (washout −), pivot as chord fraction")
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
    ui.horizontal_wrapped(|ui| {
        for (i, p) in app.wing.panels.iter().enumerate() {
            if ui.selectable_label(app.selected == i, &p.name).clicked() && app.selected != i {
                app.selected = i;
                selected_changed = true;
            }
        }
    });
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
        Grid::new("panel").num_columns(2).striped(true).show(ui, |ui| {
            ch |= row(ui, "Name", "Used for the output file names", |ui| ui.text_edit_singleline(&mut p.name).changed());
            ch |= row(ui, "Span from / to", "Half-span positions of the panel ends", |ui| {
                ui.horizontal(|ui| num(ui, &mut p.from, 1.0, " mm") | num(ui, &mut p.to, 1.0, " mm")).inner
            });
            ch |= row(ui, "Side", "Left: wing root on the root-tower side. Right: mirrored.", |ui| {
                ui.horizontal(|ui| ui.selectable_value(&mut p.side, Side::Left, "left").changed() | ui.selectable_value(&mut p.side, Side::Right, "right").changed()).inner
            });
            let b = &mut p.block;
            ch |= row(ui, "Block s (root side)", "Span position of the block face nearest the root tower", |ui| num(ui, &mut b.s_root, 1.0, " mm"));
            ch |= row(ui, "Block front x0", "Machine x of the block front (the wire enters here)", |ui| num(ui, &mut b.x0, 1.0, " mm"));
            ch |= row(ui, "Block bottom y0", "", |ui| num(ui, &mut b.y0, 1.0, " mm"));
            ch |= row(ui, "Block height / depth", "Height, and depth along x", |ui| {
                ui.horizontal(|ui| num(ui, &mut b.height, 1.0, " mm") | num(ui, &mut b.width, 1.0, " mm")).inner
            });
            ch |= row(ui, "TE inset", "Gap from the block front to the rearmost trailing edge", |ui| num(ui, &mut b.inset, 0.5, " mm"));
            ch |= row(ui, "Chord line height", "Height of the wing's y = 0 line above the block bottom; auto centres the panel", |ui| {
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

fn sheeting(ui: &mut Ui, sh: &mut Sheeting) -> bool {
    let mut ch = false;
    ui.label(RichText::new("Sheeting").strong()).on_hover_text(
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
    ui.label(RichText::new("Cutouts").strong()).on_hover_text(
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
