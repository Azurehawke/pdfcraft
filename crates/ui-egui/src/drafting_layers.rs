//! RevuCraft: drafted lines go on the Drafting layer, as Illustrator keeps artwork on its own
//! layers. Picking a drawing tool on the Draft tab makes the Drafting layer (or its Walls, Doors
//! and windows, or Dimensions sublayer) the current one, creating it the first time; putting the
//! drafting tools down brings back the layer that was current before. A layer you choose inside
//! the Drafting layer yourself is kept. Lock the Drafting layer and the linework can't be moved
//! while you mark up; flatten it to make it part of the page.

use pdfcraft_engine::layers::LayerInfo;
use pdfcraft_engine::{DocId, Edit};

use crate::draft::DraftTool;
use crate::{PdfCraftApp, QuickTool};

type Key = (u32, u16);

/// The role of the Drafting layer itself.
const DRAFTING: &str = "drafting";

/// What the routing remembers: the document, the layer it made current, the layer to bring back,
/// and the tool it routed for.
#[derive(Clone, Debug, Default)]
pub struct DraftingLayers {
    doc: Option<DocId>,
    auto: Option<Key>,
    restore: Option<Option<Key>>,
    tool: Option<DraftTool>,
}

fn key(l: &LayerInfo) -> Key {
    (l.id.num, l.id.generation)
}

/// The sublayer a tool's lines go on (`None`: the Drafting layer itself), as role and name.
pub(crate) fn sublayer(tool: DraftTool) -> Option<(&'static str, &'static str)> {
    match tool {
        DraftTool::Wall => Some(("walls", "Walls")),
        DraftTool::Door | DraftTool::Window => Some(("openings", "Doors and windows")),
        DraftTool::Dimension | DraftTool::DimLinear | DraftTool::DimRadius | DraftTool::DimDiameter => Some(("dimensions", "Dimensions")),
        _ => None,
    }
}

/// Whether `layer` is the Drafting layer or inside it.
fn in_drafting(layers: &[LayerInfo], layer: Key) -> bool {
    let mut at = layers.iter().find(|l| key(l) == layer);
    for _ in 0..32 {
        let Some(l) = at else { return false };
        if l.role.as_deref() == Some(DRAFTING) {
            return true;
        }
        at = l.parent.and_then(|p| layers.iter().find(|x| x.id == p));
    }
    false
}

/// The layer `tool` draws on, creating the Drafting layer or the sublayer when missing.
fn ensure(app: &mut PdfCraftApp, id: DocId, tool: DraftTool) -> Option<Key> {
    let find = |app: &PdfCraftApp, role: &str| app.session.layer_details(id).iter().find(|l| l.role.as_deref() == Some(role)).map(key);
    let drafting = match find(app, DRAFTING) {
        Some(k) => k,
        None => {
            let edit = Edit::AddLayerIn { name: tl!("Drafting").to_string(), color: [0.0; 3], parent: None, role: Some(DRAFTING.into()) };
            if !app.apply_edit(edit) {
                return None;
            }
            find(app, DRAFTING)?
        }
    };
    let Some((role, name)) = sublayer(tool) else { return Some(drafting) };
    // The sublayer inside this Drafting layer.
    let within = |app: &PdfCraftApp| {
        app.session
            .layer_details(id)
            .iter()
            .find(|l| l.role.as_deref() == Some(role) && l.parent.map(|p| (p.num, p.generation)) == Some(drafting))
            .map(key)
    };
    if let Some(k) = within(app) {
        return Some(k);
    }
    let edit = Edit::AddLayerIn { name: tl!(name).to_string(), color: [0.0; 3], parent: Some(drafting), role: Some(role.into()) };
    if !app.apply_edit(edit) {
        return Some(drafting);
    }
    within(app).or(Some(drafting))
}

/// Keep the current layer in step with the drafting tool in hand (once a frame, RevuCraft only).
pub(crate) fn follow_tool(app: &mut PdfCraftApp) {
    let Some((_, id)) = app.active_ids() else { return };
    let tool = match app.quick_tool {
        QuickTool::Draft(t) if app.show_ribbon && !t.modifies() => Some(t),
        _ => None,
    };
    let Some(doc) = app.session.get(id) else { return };
    let current = doc.current_layer;
    let can = doc.allows_annotation() && doc.read_only_reason.is_none();
    let state = app.drafting_layers.clone();
    match tool {
        None => {
            // The drafting tools put down: the layer from before comes back, unless another was
            // chosen meanwhile.
            if state.doc == Some(id)
                && let Some(before) = state.restore
                && current == state.auto
            {
                let _ = app.session.set_current_layer(id, before);
            }
            if state.doc.is_some() {
                app.drafting_layers = DraftingLayers::default();
            }
        }
        Some(t) => {
            if !can || (state.doc == Some(id) && state.tool == Some(t)) {
                return;
            }
            let layers = app.session.layer_details(id);
            // A layer inside Drafting chosen by hand stays.
            if let Some(c) = current
                && current != state.auto
                && in_drafting(&layers, c)
            {
                app.drafting_layers = DraftingLayers { doc: Some(id), auto: None, restore: None, tool: Some(t) };
                return;
            }
            let restore = if state.doc == Some(id) && state.restore.is_some() { state.restore } else { Some(current) };
            let Some(target) = ensure(app, id, t) else { return };
            let _ = app.session.set_current_layer(id, Some(target));
            app.drafting_layers = DraftingLayers { doc: Some(id), auto: Some(target), restore, tool: Some(t) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_have_their_sublayers() {
        assert_eq!(sublayer(DraftTool::Wall).map(|s| s.0), Some("walls"));
        assert_eq!(sublayer(DraftTool::Window).map(|s| s.0), Some("openings"));
        assert_eq!(sublayer(DraftTool::DimRadius).map(|s| s.0), Some("dimensions"));
        assert_eq!(sublayer(DraftTool::Line), None);
    }
}
