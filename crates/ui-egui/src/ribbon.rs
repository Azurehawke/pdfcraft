//! RevuCraft's tool ribbon: tools grouped by what you are doing (marking up, measuring,
//! reviewing, working on the document) under workflow tabs in the mode bar, instead of one
//! catalogue of every tool. Each button runs a registry command, so the ribbon, menus, palette
//! and shortcuts stay one set of behaviours.

use egui::{Align, Layout, Stroke, vec2};
use pdfcraft_engine::commands;

use crate::theme::Tokens;
use crate::{PdfCraftApp, QuickTool, icons, theme};

/// The workflow tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Workflow {
    #[default]
    Markup,
    Measure,
    Review,
    Document,
    /// RevuCraft's symbols, layers and photos ([`crate::revu::ribbon`]).
    Symbols,
    /// Drawing at scale ([`crate::draft::ribbon`]).
    Draft,
    /// Building fillable forms: field tools, tab order and form data.
    Forms,
}

impl Workflow {
    /// The tabs shown. Review is part of Markup now (its Comments tools are in the Markup tools
    /// panel); the variant stays so settings that saved it still read.
    pub const ALL: [Workflow; 6] = [Workflow::Markup, Workflow::Draft, Workflow::Symbols, Workflow::Measure, Workflow::Forms, Workflow::Document];

    /// The English tab name, translated where it is shown.
    pub fn label(self) -> &'static str {
        match self {
            Self::Markup => "Markup",
            Self::Measure => "Measure",
            Self::Review => "Review",
            Self::Document => "Document",
            Self::Symbols => "Symbols",
            Self::Draft => "Draft",
            Self::Forms => "Forms",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.label().eq_ignore_ascii_case(value))
    }

    /// The ribbon's groups for this tab: a caption and the commands under it.
    pub fn groups(self) -> &'static [(&'static str, &'static [&'static str])] {
        match self {
            // Markup's tools are in its tools panel; the ribbon holds the tool's options.
            Self::Markup | Self::Review => &[],
            // The measuring tools are in the Measure tools panel.
            Self::Measure => &[
                ("Scale", &["measure.scale"]),
                ("Tools", &["measure.snap", "measure.info"]),
                ("Export", &["measure.export", "measure.send_quantities"]),
            ],
            Self::Document => &[
                (
                    "Pages",
                    &[
                        "page.organize",
                        "page.insert_blank",
                        "page.insert_sheet",
                        "page.insert",
                        "page.extract",
                        "page.rotate",
                        "page.delete",
                        "page.crop",
                    ],
                ),
                ("Files", &["page.combine", "page.split"]),
                ("Edit", &["edit.edit_text", "edit.text", "edit.image", "edit.link"]),
                ("Secure", &["redact.mark", "protect.password", "sign.digital"]),
                ("Compare", &["doc.compare"]),
            ],
            Self::Forms => &[
                (
                    "Fields",
                    &[
                        "form.add.text",
                        "form.add.checkbox",
                        "form.add.radio",
                        "form.add.combo",
                        "form.add.list",
                        "form.add.date",
                        "form.add.button",
                        "form.add.image",
                        "form.add.signature",
                    ],
                ),
                ("Form", &["form.prepare", "form.detect", "form.field.properties", "form.fields"]),
                ("Tab order", &["form.tab_order.row", "form.tab_order.column", "form.tab_order.structure"]),
                ("Data", &["form.clear", "form.import_data", "form.export_data", "form.flatten"]),
            ],
            Self::Symbols | Self::Draft => &[],
        }
    }
}

/// The ribbon's icon for a command: the measuring commands share one icon in the registry, which
/// tells them apart nowhere when they sit side by side.
pub(crate) fn icon(id: &str, registry: &'static str) -> &'static str {
    match id {
        "measure.scale" => "compass",
        "measure.distance" => "ruler",
        "measure.perimeter" => "spline",
        "measure.area" => "pentagon",
        "measure.rectangle" => "square-dashed",
        "measure.angle" => "chevron-left",
        "measure.radius" => "circle-dot",
        "measure.calibrate" => "maximize-2",
        "measure.cable" => "move-right",
        "measure.snap" => "grid-3x3",
        "measure.info" => "info",
        "measure.export" => "file-spreadsheet",
        "form.prepare" => "list-checks",
        "form.fields" => "panel-right",
        _ => registry,
    }
}

/// Whether `id` names the tool that is in use now.
pub(crate) fn tool_active(app: &PdfCraftApp, id: &str) -> bool {
    if let Some(tool) = crate::comments::CommentTool::from_command(id) {
        return app.quick_tool == QuickTool::Comment(tool);
    }
    if let Some(tool) = crate::prepare::FieldTool::from_command(id) {
        return app.quick_tool == QuickTool::Field(tool);
    }
    match id.strip_prefix("measure.").and_then(crate::measure_ui::Tool::from_name) {
        Some(tool) => app.quick_tool == QuickTool::Measure(tool),
        None => false,
    }
}

/// The workflow tabs, for the mode bar.
pub fn tabs(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    for workflow in Workflow::ALL {
        if crate::widgets::mode_tab(ui, crate::i18n::t(workflow.label()), app.workflow == workflow).clicked() {
            app.workflow = workflow;
            // Reading mode hides the tools; picking a workflow brings them back.
            if app.mode == crate::Mode::Read {
                app.select_mode(crate::Mode::AllTools);
            }
        }
    }
}

/// The ribbon under the mode bar: the active tab's tool groups, each a row of buttons with its
/// caption underneath.
pub fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("ribbon")
        .exact_size(62.0)
        .frame(egui::Frame::NONE.fill(t.card).inner_margin(egui::Margin::symmetric(10, 4)).stroke(Stroke::new(1.0, t.divider)))
        .show(ui, |ui| {
            if app.workflow == Workflow::Symbols {
                crate::revu::ribbon(app, ui);
                return;
            }
            // Review is part of Markup now.
            if app.workflow == Workflow::Review {
                app.workflow = Workflow::Markup;
            }
            if app.workflow == Workflow::Markup {
                markup_options(app, ui, &t);
                return;
            }
            if app.workflow == Workflow::Draft {
                crate::draft::ribbon(app, ui);
                return;
            }
            ui.add_enabled_ui(app.active.is_some(), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let mut run = None;
                    for (i, (caption, ids)) in app.workflow.groups().iter().enumerate() {
                        if i > 0 {
                            ui.add_space(8.0);
                            let x = ui.cursor().left();
                            ui.painter().vline(x, ui.max_rect().y_range().shrink(6.0), Stroke::new(1.0, t.divider));
                            ui.add_space(8.0);
                        }
                        ui.allocate_ui_with_layout(vec2(0.0, ui.available_height()), Layout::top_down(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                for id in ids.iter() {
                                    let Some(spec) = commands::command(id) else { continue };
                                    let enabled = app.command_enabled(spec);
                                    let tip = crate::commands::command_tip(ui.ctx(), crate::i18n::t(spec.label), id);
                                    let selected = tool_active(app, id);
                                    let resp = ui.add_enabled_ui(enabled, |ui| icons::button(ui, icon(id, spec.icon), 34.0, selected, &tip)).inner;
                                    if resp.clicked() {
                                        run = Some(*id);
                                    }
                                }
                            });
                            ui.add(
                                egui::Label::new(egui::RichText::new(crate::i18n::t(caption)).font(theme::regular(11.0)).color(t.text_faint))
                                    .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        });
                    }
                    // RevuCraft: a cable run's allowance and the totals, beside its button.
                    if app.workflow == Workflow::Measure {
                        crate::cable::ribbon_group(app, ui, &t);
                    }
                    if let Some(id) = run {
                        app.execute(id);
                    }
                });
            });
        });
}

/// The Markup tab's ribbon: the chosen tool's options (a photo's text side and height), else
/// where the tools are.
fn markup_options(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if matches!(app.quick_tool, QuickTool::PhotoPin | QuickTool::PhotoCallout) {
            use pdfcraft_engine::photo_callout::TextSide;
            let s = &mut app.symbols;
            ui.label(egui::RichText::new(tl!("Photo callout")).font(theme::regular(11.0)).color(t.text_faint));
            ui.selectable_value(&mut s.callout_side, TextSide::Left, tl!("Text left"));
            ui.selectable_value(&mut s.callout_side, TextSide::Right, tl!("Text right"));
            ui.add(egui::DragValue::new(&mut s.callout_inches).range(0.5..=8.0).speed(0.05).max_decimals(2).suffix(" in"))
                .on_hover_text(tl!("The photo's height on the sheet"));
        } else {
            ui.label(egui::RichText::new(tl!("Pick a tool in the Markup tools panel on the left")).font(theme::regular(12.0)).color(t.text_muted));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every ribbon button is a registered command with an icon, so nothing on the ribbon is
    /// dead, and no command appears twice.
    #[test]
    fn every_ribbon_button_is_a_command() {
        let mut seen = std::collections::HashSet::new();
        for w in Workflow::ALL {
            for (_, ids) in w.groups() {
                for id in ids.iter() {
                    let spec = commands::command(id).unwrap_or_else(|| panic!("{id} is not a registered command"));
                    assert!(icons::exists(icon(id, spec.icon)), "{id}: missing icon {}", icon(id, spec.icon));
                    assert!(seen.insert(*id), "{id} is on the ribbon twice");
                }
            }
        }
    }

    #[test]
    fn workflows_parse_by_name() {
        for w in Workflow::ALL {
            assert_eq!(Workflow::parse(&w.label().to_lowercase()), Some(w));
        }
        assert_eq!(Workflow::parse("acrobat"), None);
    }
}
