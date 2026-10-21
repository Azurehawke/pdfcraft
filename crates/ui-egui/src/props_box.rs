//! RevuCraft's Properties box (in the dock): the selected markup's colour, fill, opacity, line
//! width, layer, text, subject and author, edited in place. Each change is applied at once as
//! one undoable step; the full Comment Properties dialog stays one click away.

use pdfcraft_engine::Edit;

use crate::comment_props::{PropsDraft, PropsTab};
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, widgets};

/// Draw the Properties box for the active document's selected markup.
pub fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((index, id)) = app.active_ids() else { return };
    let Some(doc) = app.session.get(id) else { return };
    let Some((page, ai)) = app.views.get(index).and_then(|v| v.comments.selected) else {
        app.props_box = None;
        ui.label(egui::RichText::new(tl!("Select a markup to see its properties.")).color(t.text_muted));
        return;
    };
    let Some(fresh) = doc.comment_props(page, ai) else {
        app.props_box = None;
        return;
    };
    let annotation = doc.info.annotations.iter().find(|a| a.page == page && a.index == ai).cloned();
    // The other markups selected with it take the same changes.
    let others: Vec<usize> = app
        .views
        .get(index)
        .map(|v| v.comments.group())
        .unwrap_or_default()
        .into_iter()
        .filter(|(p, i)| *p == page && *i != ai)
        .map(|(_, i)| i)
        .collect();
    let editable = doc.allows_annotation() && doc.read_only_reason.is_none();
    let layers = app.session.layer_details(id);
    let rotation = doc.symbol_rotation(page, ai);
    let callout_side = doc.callout_side(page, ai);
    let fields = doc.annotation_fields(page, ai);
    let columns = app.markup_columns.clone();
    let (new_side, new_height) = (app.symbols.callout_side, crate::revu::callout_height(&app.symbols));
    // The direction of the line or wall nearest the symbol, for lining it up.
    let wall_angle = annotation.as_ref().filter(|a| a.symbol.is_some()).and_then(|a| {
        let [x0, y0, x1, y1] = a.rect.map(f64::from);
        nearest_line_angle(doc, page, [(x0 + x1) / 2.0, (y0 + y1) / 2.0])
    });
    // A new selection, or nothing pending: show the markup as it is now.
    let reload = app.props_box.as_ref().is_none_or(|d| d.page != page || d.index != ai || d.edited == d.original);
    if reload {
        app.props_box = Some(PropsDraft { page, index: ai, tab: PropsTab::Appearance, original: fresh.clone(), edited: fresh, make_default: false });
    }
    let Some(d) = app.props_box.as_mut() else { return };
    let mut commit = false;
    let mut extra: Vec<Edit> = Vec::new();
    let mut more = false;
    let mut view_photo = false;
    let kind = annotation
        .as_ref()
        .and_then(|a| a.symbol.as_deref().map(pdfcraft_engine::symbols::display_name))
        .unwrap_or_else(|| tl!(crate::comments_panel::subtype_label(&d.edited.subtype)).to_string());
    ui.label(egui::RichText::new(kind).font(theme::semibold(14.0)));
    if !others.is_empty() {
        let n = (others.len() + 1).to_string();
        ui.label(egui::RichText::new(crate::i18n::fmt(tl!("{n} markups selected: changes apply to all of them"), &[("n", &n)])).color(t.text_muted));
    }
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt("props-box").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_enabled_ui(editable && !d.edited.locked, |ui| {
            egui::Grid::new("props-box-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                if d.edited.restylable {
                    ui.label(tl!("Colour"));
                    if let Some(c) = crate::comments::swatch_grid(ui, d.edited.color) {
                        d.edited.color = Some(c);
                        commit = true;
                    }
                    ui.end_row();
                    if d.edited.fillable {
                        ui.label(tl!("Fill colour"));
                        if let Some(f) = crate::comments::fill_picker(ui, d.edited.fill) {
                            d.edited.fill = f;
                            commit = true;
                        }
                        ui.end_row();
                    }
                    ui.label(tl!("Opacity"));
                    let mut pct = d.edited.opacity * 100.0;
                    let r = ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%"));
                    if r.changed() {
                        d.edited.opacity = pct / 100.0;
                    }
                    commit |= r.drag_stopped() || (r.changed() && !r.dragged());
                    ui.end_row();
                    // Text boxes and callouts may go borderless; lines keep a visible stroke.
                    let min_width = if d.edited.font_size.is_some() { 0.0 } else { 0.25 };
                    if let Some(w) = d.edited.width.as_mut() {
                        ui.label(tl!("Thickness"));
                        let r = ui.add(egui::DragValue::new(w).range(min_width..=24.0).speed(0.1).suffix(" pt"));
                        commit |= r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus());
                        ui.end_row();
                    }
                }
                // Layer.
                if let Some(a) = annotation.as_ref() {
                    ui.label(tl!("Layer"));
                    let mut choice = a.layer;
                    let name_of = |c: Option<(u32, u16)>| {
                        c.and_then(|c| layers.iter().find(|l| (l.id.num, l.id.generation) == c))
                            .map_or_else(|| tl!("No layer").to_string(), |l| l.name.clone())
                    };
                    egui::ComboBox::from_id_salt("props-layer").selected_text(name_of(a.layer)).width(150.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut choice, None, tl!("No layer"));
                        for l in &layers {
                            ui.selectable_value(&mut choice, Some((l.id.num, l.id.generation)), l.name.as_str());
                        }
                    });
                    if choice != a.layer {
                        extra.push(Edit::SetAnnotationLayer { page, index: ai, layer: choice });
                    }
                    ui.end_row();
                    // The text (a symbol's label, a note's comment).
                    ui.label(if a.symbol.is_some() { tl!("Label") } else { tl!("Text") });
                    let text_id = egui::Id::new(("props-text", page, ai));
                    let mut text = ui.data(|m| m.get_temp::<String>(text_id)).unwrap_or_else(|| a.contents.clone().unwrap_or_default());
                    let r = ui.add(egui::TextEdit::multiline(&mut text).desired_rows(2).desired_width(170.0));
                    if r.lost_focus() {
                        if text != a.contents.clone().unwrap_or_default() {
                            // A symbol's label is drawn under it, so the symbol is redrawn.
                            if a.symbol.is_some() {
                                extra.push(Edit::EditSymbol { page, index: ai, label: Some(text.clone()), rotation: None });
                            } else {
                                extra.push(Edit::SetAnnotationContents { page, index: ai, text: text.clone() });
                            }
                        }
                        ui.data_mut(|m| m.remove::<String>(text_id));
                    } else if r.has_focus() {
                        ui.data_mut(|m| m.insert_temp(text_id, text));
                    }
                    ui.end_row();
                    // A symbol's turn, and turning it to line up with the nearest wall or line.
                    if a.symbol.is_some() {
                        ui.label(tl!("Rotation"));
                        ui.horizontal(|ui| {
                            let mut turn = rotation;
                            let r = ui.add(egui::DragValue::new(&mut turn).range(-360.0..=360.0).speed(1.0).suffix("°"));
                            if (r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus()))
                                && (turn - rotation).abs() > 1e-6
                            {
                                extra.push(Edit::EditSymbol { page, index: ai, label: None, rotation: Some(turn) });
                            }
                            if widgets::pill_button(ui, tl!("Align to nearest wall"), false)
                                .on_hover_text(tl!("Turn the symbol to line up with the nearest wall or line"))
                                .clicked()
                                && let Some(t) = wall_angle
                            {
                                extra.push(Edit::EditSymbol { page, index: ai, label: None, rotation: Some(t) });
                            }
                        });
                        ui.end_row();
                    }
                    // A photo callout: the side its description is on.
                    if let Some(side) = callout_side {
                        use pdfcraft_engine::photo_callout::TextSide;
                        ui.label(tl!("Description"));
                        ui.horizontal(|ui| {
                            let mut choice = side;
                            ui.selectable_value(&mut choice, TextSide::Left, tl!("Text left"));
                            ui.selectable_value(&mut choice, TextSide::Right, tl!("Text right"));
                            if choice != side {
                                extra.push(Edit::SetCalloutSide { page, index: ai, side: choice });
                            }
                        });
                        ui.end_row();
                    }
                    // A photo pin can become a photo callout.
                    if a.photo {
                        ui.label(tl!("Photo"));
                        if widgets::pill_button(ui, tl!("Make a callout"), false)
                            .on_hover_text(tl!("Show the photo in a box with its description, pointing at this spot"))
                            .clicked()
                        {
                            extra.push(Edit::PinToCallout { page, index: ai, side: new_side, photo_height: new_height });
                        }
                        ui.end_row();
                    }
                }
                // Your own columns.
                for col in &columns {
                    let current = fields.iter().find(|(n, _)| *n == col.name).map(|(_, v)| v.clone()).unwrap_or_default();
                    ui.label(col.name.as_str());
                    if col.choices.is_empty() {
                        let id = egui::Id::new(("props-field", page, ai, col.name.as_str()));
                        let mut text = ui.data(|m| m.get_temp::<String>(id)).unwrap_or_else(|| current.clone());
                        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(170.0));
                        if r.lost_focus() {
                            if text.trim() != current {
                                extra.push(Edit::SetAnnotationField { page, index: ai, name: col.name.clone(), value: text.clone() });
                            }
                            ui.data_mut(|m| m.remove::<String>(id));
                        } else if r.has_focus() {
                            ui.data_mut(|m| m.insert_temp(id, text));
                        }
                    } else {
                        let mut choice = current.clone();
                        egui::ComboBox::from_id_salt(("props-choice", col.name.as_str())).selected_text(choice.as_str()).width(170.0).show_ui(
                            ui,
                            |ui| {
                                ui.selectable_value(&mut choice, String::new(), "—");
                                for c in &col.choices {
                                    ui.selectable_value(&mut choice, c.clone(), c.as_str());
                                }
                                if !current.is_empty() && !col.choices.contains(&current) {
                                    ui.selectable_value(&mut choice, current.clone(), current.as_str());
                                }
                            },
                        );
                        if choice != current {
                            extra.push(Edit::SetAnnotationField { page, index: ai, name: col.name.clone(), value: choice });
                        }
                    }
                    ui.end_row();
                }
                ui.label(tl!("Subject"));
                let r = ui.add(egui::TextEdit::singleline(&mut d.edited.subject).desired_width(170.0));
                commit |= r.lost_focus();
                ui.end_row();
                ui.label(tl!("Author"));
                let r = ui.add(egui::TextEdit::singleline(&mut d.edited.author).desired_width(170.0));
                commit |= r.lost_focus();
                ui.end_row();
            });
        });
        ui.add_space(6.0);
        ui.add_enabled_ui(editable, |ui| {
            if ui.add(crate::widgets::checkbox(&mut d.edited.locked, tl!("Locked"))).changed() {
                commit = true;
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if widgets::pill_button(ui, tl!("More properties…"), false).clicked() {
                more = true;
            }
            if (callout_side.is_some() || annotation.as_ref().is_some_and(|a| a.photo))
                && widgets::pill_button(ui, tl!("View photo"), false).clicked()
            {
                view_photo = true;
            }
        });
    });
    let mut edits = if commit { crate::comment_props::edits(d) } else { Vec::new() };
    if commit {
        d.original = d.edited.clone();
    }
    edits.extend(extra);
    if !others.is_empty() {
        let mut more_edits = Vec::new();
        for &other in &others {
            let props = doc.comment_props(page, other);
            let fit = props.as_ref().map_or((false, false, false), |p| (p.restylable, p.width.is_some(), p.fillable));
            more_edits.extend(edits.iter().filter_map(|e| retarget(e, other, fit)));
        }
        edits.extend(more_edits);
    }
    match edits.len() {
        0 => {}
        1 => {
            if let Some(e) = edits.pop() {
                app.apply_edit(e);
            }
        }
        _ => {
            app.apply_edit(Edit::Batch { label: "Change properties".into(), edits });
        }
    }
    if more {
        app.open_comment_props(page, ai);
    }
    if view_photo {
        app.open_photo(page, ai);
    }
}

/// The same change for another selected markup, where it applies to that kind of markup
/// (`fit`: it can be restyled, has a line width, can be filled). Text and symbol changes stay
/// with the one markup.
fn retarget(e: &Edit, index: usize, (restylable, has_width, fillable): (bool, bool, bool)) -> Option<Edit> {
    match e {
        Edit::StyleAnnotation { page, color, opacity, width, .. } if restylable => {
            Some(Edit::StyleAnnotation { page: *page, index, color: *color, opacity: *opacity, width: width.filter(|_| has_width), endings: None })
        }
        Edit::FillAnnotation { page, fill, .. } if fillable => Some(Edit::FillAnnotation { page: *page, index, fill: *fill }),
        Edit::LockAnnotation { page, locked, .. } => Some(Edit::LockAnnotation { page: *page, index, locked: *locked }),
        Edit::SetAnnotationLayer { page, layer, .. } => Some(Edit::SetAnnotationLayer { page: *page, index, layer: *layer }),
        Edit::SetAnnotationField { page, name, value, .. } => {
            Some(Edit::SetAnnotationField { page: *page, index, name: name.clone(), value: value.clone() })
        }
        _ => None,
    }
}

/// The angle (degrees, -90 to 90) of the markup line nearest `p` on `page`, for lining a symbol up
/// with a wall.
pub(crate) fn nearest_line_angle(doc: &pdfcraft_engine::Document, page: usize, p: [f64; 2]) -> Option<f64> {
    nearest_line(doc, page, p).map(|(_, a)| a)
}

/// The distance to the markup line nearest `p` on `page`, and its angle (degrees, -90 to 90).
pub(crate) fn nearest_line(doc: &pdfcraft_engine::Document, page: usize, p: [f64; 2]) -> Option<(f64, f64)> {
    let mut best: Option<(f64, f64)> = None;
    for g in doc.markup_geometry(page) {
        for (a, b) in g.segments() {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dy * dy;
            if len2 < 1e-12 {
                continue;
            }
            let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
            let d = (p[0] - a[0] - dx * t).hypot(p[1] - a[1] - dy * t);
            if best.is_none_or(|(bd, _)| d < bd) {
                let mut angle = dy.atan2(dx).to_degrees();
                if angle > 90.0 {
                    angle -= 180.0;
                } else if angle <= -90.0 {
                    angle += 180.0;
                }
                best = Some((d, angle));
            }
        }
    }
    best
}
