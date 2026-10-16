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
    pub const ALL: [Workflow; 7] =
        [Workflow::Markup, Workflow::Draft, Workflow::Symbols, Workflow::Measure, Workflow::Review, Workflow::Forms, Workflow::Document];

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
            Self::Markup => &[
                ("Text markup", &["comment.highlight", "comment.underline", "comment.strikeout", "comment.caret", "comment.replace"]),
                ("Draw", &["comment.ink", "comment.eraser"]),
                (
                    "Shapes",
                    &["comment.line", "comment.arrow", "comment.square", "comment.circle", "comment.polygon", "comment.polyline", "comment.cloud"],
                ),
                ("Text", &["comment.note", "comment.freetext", "comment.callout"]),
                ("Stamp", &["comment.stamp", "comment.attach"]),
            ],
            Self::Measure => &[
                ("Scale", &["measure.scale"]),
                ("Measure", &["measure.distance", "measure.perimeter", "measure.area", "measure.rectangle", "measure.angle", "measure.radius"]),
                ("Tools", &["measure.snap", "measure.info"]),
                ("Export", &["measure.export"]),
            ],
            Self::Review => &[
                ("Comments", &["comment.list", "comment.summarize", "comment.hide_all", "comment.flatten"]),
                ("Exchange", &["comment.import", "comment.export"]),
                ("Compare", &["doc.compare"]),
            ],
            Self::Document => &[
                ("Pages", &["page.organize", "page.insert", "page.extract", "page.rotate", "page.delete", "page.crop"]),
                ("Files", &["page.combine", "page.split"]),
                ("Edit", &["edit.edit_text", "edit.text", "edit.image", "edit.link"]),
                ("Secure", &["redact.mark", "protect.password", "sign.digital"]),
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
fn icon(id: &str, registry: &'static str) -> &'static str {
    match id {
        "measure.scale" => "compass",
        "measure.distance" => "ruler",
        "measure.perimeter" => "spline",
        "measure.area" => "pentagon",
        "measure.rectangle" => "square-dashed",
        "measure.angle" => "chevron-left",
        "measure.radius" => "circle-dot",
        "measure.snap" => "grid-3x3",
        "measure.info" => "info",
        "measure.export" => "file-spreadsheet",
        "form.prepare" => "list-checks",
        "form.fields" => "panel-right",
        _ => registry,
    }
}

/// Whether `id` names the tool that is in use now.
fn tool_active(app: &PdfCraftApp, id: &str) -> bool {
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
                            ui.label(egui::RichText::new(crate::i18n::t(caption)).font(theme::regular(11.0)).color(t.text_faint));
                        });
                    }
                    // RevuCraft: dimensions are added to existing drawings too, so Markup has them
                    // as well as the Draft tools panel.
                    if app.workflow == Workflow::Markup {
                        ui.add_space(8.0);
                        let x = ui.cursor().left();
                        ui.painter().vline(x, ui.max_rect().y_range().shrink(6.0), Stroke::new(1.0, t.divider));
                        ui.add_space(8.0);
                        ui.allocate_ui_with_layout(vec2(0.0, ui.available_height()), Layout::top_down(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                for tool in crate::draft::DraftTool::DIMENSIONS {
                                    let selected = app.quick_tool == QuickTool::Draft(tool);
                                    if crate::cad_icons::button(ui, tool.icon(), 34.0, selected, crate::i18n::t(tool.label())).clicked() {
                                        app.quick_tool = if selected { QuickTool::Select } else { QuickTool::Draft(tool) };
                                    }
                                }
                            });
                            ui.label(egui::RichText::new(crate::i18n::t("Dimensions")).font(theme::regular(11.0)).color(t.text_faint));
                        });
                    }
                    if let Some(id) = run {
                        app.execute(id);
                    }
                });
            });
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
