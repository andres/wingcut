//! Central area: the cut preview (both faces, tower paths, block, limits)
//! and a planform view of the whole wing.

use crate::app::{App, PreviewData, ViewTab};
use eframe::egui::{Color32, RichText, Ui};
use egui_plot::{Legend, Line, LineStyle, MarkerShape, Plot, PlotPoints, Points, Polygon, Text};
use wingcut::config::Side;
use wingcut::geom::P2;

const ROOT_FACE: Color32 = Color32::from_rgb(31, 79, 209);
const TIP_FACE: Color32 = Color32::from_rgb(209, 47, 47);
const ROOT_TOWER: Color32 = Color32::from_rgb(42, 157, 75);
const TIP_TOWER: Color32 = Color32::from_rgb(224, 123, 0);
const BLOCK: Color32 = Color32::from_rgb(201, 164, 0);
const LIMITS: Color32 = Color32::from_gray(140);

fn pts(v: &[P2]) -> PlotPoints<'static> {
    v.iter().map(|p| [p.x, p.y]).collect()
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> PlotPoints<'static> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]].into()
}

pub fn show(ui: &mut Ui, app: &mut App) {
    // Wraps on narrow windows instead of widening the panel.
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut app.view_tab, ViewTab::Cut, "Cut preview");
        ui.selectable_value(&mut app.view_tab, ViewTab::Planform, "Planform")
            .on_hover_text("The whole wing from above");
        ui.selectable_value(&mut app.view_tab, ViewTab::Front, "Front view")
            .on_hover_text("The whole wing from the front: dihedral");
        crate::help::button(ui, "views");
        ui.separator();
        if app.view_tab == ViewTab::Cut {
            let l = &mut app.layers;
            ui.checkbox(&mut l.nominal, "nominal");
            ui.checkbox(&mut l.kerfed, "with kerf");
            ui.checkbox(&mut l.towers, "tower paths");
            ui.checkbox(&mut l.block, "block");
            ui.checkbox(&mut l.limits, "axis limits");
            ui.label(RichText::new("tips").small().weak())
                .on_hover_text("Drag to pan, scroll to zoom, double-click to fit. Click a legend entry to hide it.");
        }
    });
    match app.view_tab {
        ViewTab::Cut => match &app.preview {
            Some(Ok(p)) => cut(ui, app, p),
            Some(Err(_)) | None => {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Fix the error below to see the preview.").weak())
                });
            }
        },
        ViewTab::Planform => planform(ui, app),
        ViewTab::Front => front(ui, app),
    }
}

fn cut(ui: &mut Ui, app: &App, p: &PreviewData) {
    let plan = &p.plan;
    let l = &app.layers;
    let b = &p.panel.block;
    let m = &p.machine;
    let [f0, f1] = &plan.faces;
    let (c0, c1) = if f0.is_wing_root {
        (ROOT_FACE, TIP_FACE)
    } else {
        (TIP_FACE, ROOT_FACE)
    };
    Plot::new(("cut", &p.panel.name))
        .data_aspect(1.0)
        .legend(Legend::default())
        .x_axis_label("x (mm)")
        .y_axis_label("y (mm)")
        .show(ui, |pu| {
            if l.limits {
                pu.polygon(
                    Polygon::new(
                        "axis limits",
                        rect(m.limits.x[0], m.limits.y[0], m.limits.x[1], m.limits.y[1]),
                    )
                    .stroke((1.0, LIMITS))
                    .fill_color(Color32::TRANSPARENT)
                    .style(LineStyle::dashed_loose()),
                );
            }
            if l.block {
                pu.polygon(
                    Polygon::new(
                        "foam block",
                        rect(b.x0, b.y0, b.x0 + b.width, b.y0 + b.height),
                    )
                    .stroke((1.0, BLOCK))
                    .fill_color(Color32::from_rgba_unmultiplied(255, 220, 90, 40)),
                );
            }
            if l.towers {
                pu.line(
                    Line::new(
                        format!("root tower ({}/{})", m.axes[0], m.axes[1]),
                        pts(&plan.root_tower),
                    )
                    .color(ROOT_TOWER)
                    .width(1.0),
                );
                pu.line(
                    Line::new(
                        format!("tip tower ({}/{})", m.axes[2], m.axes[3]),
                        pts(&plan.tip_tower),
                    )
                    .color(TIP_TOWER)
                    .width(1.0),
                );
            }
            for (f, c) in [(f0, c0), (f1, c1)] {
                let name = format!("face s={:.0} ({})", f.s, f.role());
                if l.nominal {
                    pu.line(
                        Line::new(format!("{name} nominal"), pts(&f.nominal))
                            .color(c)
                            .width(1.0)
                            .style(LineStyle::dashed_dense()),
                    );
                }
                if l.kerfed {
                    pu.line(
                        Line::new(format!("{name}, kerf {:.2} mm", f.kerf), pts(&f.path))
                            .color(c)
                            .width(1.8),
                    );
                }
            }
            if !p.violations.is_empty() {
                let bad: PlotPoints = p
                    .violations
                    .iter()
                    .filter_map(|v| {
                        let path = match v.tower {
                            "root" => &plan.root_tower,
                            "tip" => &plan.tip_tower,
                            _ => return None,
                        };
                        v.index.and_then(|i| path.get(i)).map(|q| [q.x, q.y])
                    })
                    .collect();
                pu.points(
                    Points::new("outside limits", bad)
                        .color(Color32::RED)
                        .radius(3.0)
                        .shape(MarkerShape::Circle),
                );
            }
            // Live wire position from the machine (root and tip tower).
            if let Some([r, t]) = app
                .control
                .mpos
                .as_deref()
                .and_then(|p| crate::control::towers(m, p))
            {
                let col = Color32::from_rgb(255, 0, 200);
                pu.line(
                    Line::new("wire now", vec![r, t])
                        .color(col)
                        .width(1.0)
                        .style(LineStyle::dashed_dense()),
                );
                pu.points(
                    Points::new("wire now", vec![r, t])
                        .color(col)
                        .radius(5.0)
                        .shape(MarkerShape::Diamond),
                );
            }
        });
}

/// Which panel (if any) covers span position `x` (left wing negative).
fn panel_at(app: &App, x: f64) -> Option<usize> {
    app.wing.panels.iter().position(|p| {
        let k = if p.side == Side::Left { -1.0 } else { 1.0 };
        let (a, b) = (k * p.from, k * p.to);
        x >= a.min(b) && x <= a.max(b)
    })
}

fn sorted_stations(app: &App) -> Vec<wingcut::config::Station> {
    let mut st = app.wing.stations.clone();
    st.sort_by(|a, b| a.span.total_cmp(&b.span));
    st
}

/// Span positions of all stations on both halves, left (negative) first.
fn both_halves(st: &[wingcut::config::Station]) -> Vec<f64> {
    st.iter()
        .rev()
        .map(|s| -s.span)
        .chain(st.iter().map(|s| s.span))
        .collect()
}

fn planform(ui: &mut Ui, app: &mut App) {
    // Top view of the whole wing: left panels at negative span, right panels
    // at positive span, chord downwards (LE on top).
    let st = sorted_stations(app);
    if st.len() < 2 {
        ui.label("Add at least two stations.");
        return;
    }
    let le = |s: f64| interp(&st, s.abs(), |x| -x.le_offset);
    let te = |s: f64| interp(&st, s.abs(), |x| -(x.le_offset + x.chord));
    let pr = Plot::new("planform")
        .data_aspect(1.0)
        .legend(Legend::default())
        .x_axis_label("span (mm): left wing ◀ 0 ▶ right wing · click a panel to select it")
        .y_axis_label("chord (mm), leading edge on top")
        .show(ui, |pu| {
            for (i, p) in app.wing.panels.iter().enumerate() {
                let sel = i == app.selected;
                let k = if p.side == Side::Left { -1.0 } else { 1.0 };
                let (a, b) = (k * p.from, k * p.to);
                let quad = vec![[a, le(a)], [b, le(b)], [b, te(b)], [a, te(a)]];
                let alpha = if sel { 110 } else { 30 };
                pu.polygon(
                    Polygon::new(
                        if sel { "selected panel" } else { "panels" },
                        PlotPoints::from(quad),
                    )
                    .fill_color(Color32::from_rgba_unmultiplied(31, 79, 209, alpha))
                    .stroke((if sel { 2.0 } else { 0.5 }, ROOT_FACE)),
                );
                let mid = (a + b) / 2.0;
                pu.text(Text::new(
                    "panel names",
                    [mid, (le(mid) + te(mid)) / 2.0].into(),
                    RichText::new(&p.name).strong(),
                ));
            }
            let span = both_halves(&st);
            pu.line(
                Line::new(
                    "leading edge",
                    span.iter().map(|&s| [s, le(s)]).collect::<PlotPoints>(),
                )
                .color(Color32::GRAY)
                .width(1.5),
            );
            pu.line(
                Line::new(
                    "trailing edge",
                    span.iter().map(|&s| [s, te(s)]).collect::<PlotPoints>(),
                )
                .color(Color32::GRAY)
                .width(1.5),
            );
            for &s in &span {
                pu.line(
                    Line::new("stations", vec![[s, le(s)], [s, te(s)]])
                        .color(LIMITS)
                        .style(LineStyle::dashed_dense()),
                );
                pu.text(Text::new(
                    "station positions",
                    [s, le(s) + 25.0].into(),
                    RichText::new(format!("{:.0}", s.abs())).small(),
                ));
            }
        });
    if pr.response.clicked()
        && let Some(pos) = pr.response.interact_pointer_pos()
        && let Some(i) = panel_at(app, pr.transform.value_from_position(pos).x)
        && i != app.selected
    {
        app.selected = i;
        app.dirty = true;
    }
}

fn front(ui: &mut Ui, app: &mut App) {
    // Front view: dihedral offset along the span, both halves.
    let st = sorted_stations(app);
    if st.len() < 2 {
        ui.label("Add at least two stations.");
        return;
    }
    let up = |s: f64| interp(&st, s.abs(), |x| x.dihedral_offset);
    let pr = Plot::new("front")
        .legend(Legend::default())
        .x_axis_label("span (mm): left wing ◀ 0 ▶ right wing · click a panel to select it")
        .y_axis_label("dihedral offset (mm), vertical scale exaggerated")
        .show(ui, |pu| {
            let span = both_halves(&st);
            pu.line(
                Line::new(
                    "wing (reference line)",
                    span.iter().map(|&s| [s, up(s)]).collect::<PlotPoints>(),
                )
                .color(Color32::GRAY)
                .width(1.5),
            );
            for (i, p) in app.wing.panels.iter().enumerate() {
                let sel = i == app.selected;
                let k = if p.side == Side::Left { -1.0 } else { 1.0 };
                let (a, b) = (k * p.from, k * p.to);
                pu.line(
                    Line::new(
                        if sel { "selected panel" } else { "panels" },
                        vec![[a, up(a)], [b, up(b)]],
                    )
                    .color(if sel {
                        ROOT_FACE
                    } else {
                        Color32::from_rgba_unmultiplied(31, 79, 209, 90)
                    })
                    .width(if sel { 6.0 } else { 3.0 }),
                );
                let mid = (a + b) / 2.0;
                pu.text(Text::new(
                    "panel names",
                    [mid, up(mid)].into(),
                    RichText::new(&p.name).strong(),
                ));
            }
            pu.points(
                Points::new(
                    "stations",
                    span.iter().map(|&s| [s, up(s)]).collect::<PlotPoints>(),
                )
                .radius(3.0)
                .color(LIMITS),
            );
        });
    if pr.response.clicked()
        && let Some(pos) = pr.response.interact_pointer_pos()
        && let Some(i) = panel_at(app, pr.transform.value_from_position(pos).x)
        && i != app.selected
    {
        app.selected = i;
        app.dirty = true;
    }
}

/// Linear interpolation of a station value along the span.
fn interp(
    st: &[wingcut::config::Station],
    s: f64,
    f: impl Fn(&wingcut::config::Station) -> f64,
) -> f64 {
    let i = st.partition_point(|x| x.span <= s).clamp(1, st.len() - 1);
    let (a, b) = (&st[i - 1], &st[i]);
    let t = if b.span > a.span {
        ((s - a.span) / (b.span - a.span)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    f(a) + (f(b) - f(a)) * t
}
