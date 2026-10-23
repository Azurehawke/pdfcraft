//! RevuCraft: Make symbol. The drawn lines and shapes you select become a symbol of your own,
//! named and listed under a trade (or My symbols), ready to place, count and list like the
//! library's. The drawing itself stays as it is.

use pdfcraft_engine::DocId;
use pdfcraft_engine::symbols::{CustomArt, Trade};

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons};

/// Make symbol, waiting for its name and trade.
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    pub doc: DocId,
    pub page: usize,
    pub indices: Vec<usize>,
    pub name: String,
    pub trade: Option<Trade>,
}

/// Whether a markup of this subtype can go into a symbol: a line or shape.
pub(crate) fn drawable(subtype: &str) -> bool {
    matches!(subtype, "Line" | "PolyLine" | "Polygon" | "Square" | "Circle" | "Ink")
}

/// What the dialog's buttons asked for.
enum Act {
    Make,
    Cancel,
}

/// The Make symbol dialog, while one is pending.
pub(crate) fn dialog(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(p) = app.make_symbol.as_mut() else { return };
    let t = Tokens::get(ctx);
    let mut act: Option<Act> = None;
    let modal = egui::Modal::new(egui::Id::new("make_symbol")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.horizontal(|ui| {
            ui.add(icons::image("stamp", 20.0, t.accent));
            ui.label(egui::RichText::new(tl!("Make symbol")).font(theme::semibold(16.0)));
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(tl!(
                "The selected lines and shapes become a symbol of your own, kept with your symbols for every drawing. The drawing stays as it is."
            ))
            .color(t.text_muted),
        );
        ui.add_space(8.0);
        ui.label(tl!("Name"));
        let field = ui.add(egui::TextEdit::singleline(&mut p.name).hint_text(tl!("Symbol name")).desired_width(f32::INFINITY));
        field.request_focus();
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            act = Some(Act::Make);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(tl!("List under"));
            let shown = p.trade.map_or(tl!("My symbols"), |x| tl!(x.name()));
            egui::ComboBox::from_id_salt("make-symbol-trade").selected_text(shown).show_ui(ui, |ui| {
                ui.selectable_value(&mut p.trade, None, tl!("My symbols"));
                for trade in Trade::ALL {
                    ui.selectable_value(&mut p.trade, Some(trade), tl!(trade.name()));
                }
            });
        });
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(!p.name.trim().is_empty(), egui::Button::new(tl!("Make symbol"))).clicked() {
                act = Some(Act::Make);
            }
            if ui.button(tl!("Cancel")).clicked() {
                act = Some(Act::Cancel);
            }
        });
    });
    if modal.should_close() && act.is_none() {
        act = Some(Act::Cancel);
    }
    match act {
        None => {}
        Some(Act::Cancel) => app.make_symbol = None,
        Some(Act::Make) => {
            let Some(p) = app.make_symbol.take() else { return };
            let name = pdfcraft_engine::symbols::clean_label(&p.name);
            if name.is_empty() {
                app.make_symbol = Some(p);
                return;
            }
            let art =
                app.session.get(p.doc).ok_or_else(|| tl!("The document is closed.").to_string()).and_then(|d| d.symbol_art_from(p.page, &p.indices));
            match art {
                Ok(art) => app.keep_my_symbol(CustomArt { name, art, trade: p.trade }),
                Err(e) => app.notify(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lines_and_shapes_make_symbols() {
        for s in ["Line", "PolyLine", "Polygon", "Square", "Circle", "Ink"] {
            assert!(drawable(s), "{s}");
        }
        for s in ["FreeText", "Text", "Stamp", "Highlight", ""] {
            assert!(!drawable(s), "{s}");
        }
    }
}
