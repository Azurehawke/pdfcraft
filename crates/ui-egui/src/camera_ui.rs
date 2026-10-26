//! Scan & OCR ▸ Enhance ▸ Camera image: four corner handles on a page, dragged onto the edges of
//! a photographed document; Enhance page straightens what they enclose
//! (`Session::enhance_camera_page`, one undoable step).

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Shape, Stroke, vec2};

use crate::canvas::{DocView, PageXform, ViewAction};
use crate::page_pointer::PagePointer;
use crate::theme::Tokens;
pub use pdfcraft_engine::camera_clean::Look;

/// How far in from the page's edges the corners start, as a share of its size.
const INSET: f32 = 0.05;
/// How close (screen points) the pointer must be to pick up a corner.
const GRAB: f32 = 12.0;
/// The slider's top.
pub const MAX_LEVEL: u8 = pdfcraft_engine::camera_clean::MAX_LEVEL;
const BLUE: Color32 = Color32::from_rgb(0x14, 0x73, 0xE6);

/// The quadrilateral being placed: its page, its corners in view space (points, y down) and the
/// corner being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraQuad {
    pub page: usize,
    pub corners: [[f32; 2]; 4],
    pub grab: Option<usize>,
}

impl CameraQuad {
    /// The page `page` (view size `w` × `h` points) inset by [`INSET`] on every side.
    pub fn inset(page: usize, w: f32, h: f32) -> Self {
        let (dx, dy) = (w * INSET, h * INSET);
        Self { page, corners: [[dx, dy], [w - dx, dy], [w - dx, h - dy], [dx, h - dy]], grab: None }
    }
}

/// The clean-up level a page is enhanced at before the slider is touched.
pub const DEFAULT_LEVEL: u8 = 50;

/// A page just enhanced: what the bar's slider needs to enhance it again at another level
/// (from the straightened photo, keeping the turns made since).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraDone {
    pub page: usize,
    /// The corners it was straightened from, in PDF user space.
    pub corners: [[f64; 2]; 4],
    /// The look and slider level chosen, and the ones the page shows.
    pub look: Look,
    pub level: u8,
    pub applied: (Look, u8),
    /// Quarter turns made since, in degrees (clockwise positive).
    pub turns: i64,
    /// The undo steps the enhancement and the turns took.
    pub steps: usize,
}

/// The eraser's brush sizes, as a share of the page's width (radius).
pub const ERASER_SIZES: [f32; 3] = [0.008, 0.018, 0.035];

/// Eraser: brushing marks out of an enhanced page with the paper's colour. Each stroke is applied
/// when the pen lifts (one undoable step).
#[derive(Clone, Debug, Default)]
pub struct CameraErase {
    pub page: usize,
    /// The stroke being drawn, in page coordinates as displayed (0–1).
    pub stroke: Vec<[f32; 2]>,
    /// Index into [`ERASER_SIZES`].
    pub size: usize,
}

impl CameraErase {
    pub fn radius(&self) -> f32 {
        ERASER_SIZES.get(self.size).copied().unwrap_or(ERASER_SIZES[1])
    }
}

/// Enhance all pages in progress: a worker straightens the pages one after another
/// (`pdfcraft_engine::camera::CameraBatch`), and the pages replace the document's in one
/// undoable step once all are done.
pub struct CameraAll {
    pub total: usize,
    /// Pages back from the worker so far.
    pub finished: usize,
    pub rx: std::sync::mpsc::Receiver<Result<pdfcraft_engine::camera::CameraPage, String>>,
    pub pages: Vec<pdfcraft_engine::camera::CameraPage>,
    pub failed: Vec<String>,
    /// Asks the worker to stop (Cancel).
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The document as the worker read it: the pages are only used if it hasn't changed since.
    pub bytes: std::sync::Arc<Vec<u8>>,
}

impl std::fmt::Debug for CameraAll {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CameraAll").field("total", &self.total).field("finished", &self.finished).finish_non_exhaustive()
    }
}

fn screen(xf: &PageXform, c: [f32; 2]) -> Pos2 {
    xf.norm_to_screen(c[0] / xf.pw.max(1e-3), c[1] / xf.ph.max(1e-3))
}

/// Corner dragging on one page. A press on a page without the quadrilateral moves it there.
pub(crate) fn page_input(ui: &egui::Ui, resp: &egui::Response, xf: &PageXform, ptr: &PagePointer, page: usize, view: &mut DocView, allowed: bool) {
    let (pointer, origin) = (ptr.pos, ptr.origin);
    if !allowed {
        if pointer.is_some_and(|p| xf.rect.contains(p)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
        }
        return;
    }
    // Eraser: the pen (or mouse) brushes, and the stroke is applied when it lifts.
    if let Some(erase) = view.camera_erase.as_mut() {
        if erase.page == page {
            if let Some(p) = pointer.filter(|p| xf.rect.contains(*p)) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                if resp.drag_started() || resp.dragged() {
                    let (u, v) = xf.screen_to_norm(p);
                    if erase.stroke.len() < 4096 {
                        erase.stroke.push([u, v]);
                    }
                }
            }
            if (resp.drag_stopped() || resp.clicked()) && !erase.stroke.is_empty() {
                if resp.clicked()
                    && let Some(p) = pointer
                {
                    let (u, v) = xf.screen_to_norm(p);
                    erase.stroke.push([u, v]);
                }
                view.pending_action = Some(ViewAction::CameraErase);
            } else if resp.clicked()
                && let Some(p) = pointer.filter(|p| xf.rect.contains(*p))
            {
                // A tap erases a dot.
                let (u, v) = xf.screen_to_norm(p);
                erase.stroke.push([u, v]);
                view.pending_action = Some(ViewAction::CameraErase);
            }
        }
        return;
    }
    // Split pages: drag the line across the spread.
    if let Some((split_page, fraction)) = view.camera_split.as_mut() {
        if *split_page == page
            && (resp.dragged() || resp.drag_started() || resp.clicked())
            && let Some(p) = pointer
            && xf.rect.contains(p)
        {
            let (u, _) = xf.screen_to_norm(p);
            *fraction = u.clamp(0.05, 0.95);
        }
        if *split_page == page && pointer.is_some_and(|p| xf.rect.contains(p)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        return;
    }
    // Once the page is enhanced, the bar's buttons take over.
    if view.camera_done.is_some() {
        return;
    }
    // The tool starts with the corners on the current page.
    if view.camera.is_none() && page == view.current {
        view.camera = Some(CameraQuad::inset(page, xf.pw, xf.ph));
    }
    if resp.drag_started() || resp.clicked() {
        let pressed = origin.filter(|o| xf.rect.contains(*o));
        if pressed.is_some() && view.camera.is_none_or(|q| q.page != page) {
            view.camera = Some(CameraQuad::inset(page, xf.pw, xf.ph));
        }
        if resp.drag_started()
            && let (Some(o), Some(q)) = (pressed, view.camera.as_mut())
        {
            q.grab = (0..4)
                .map(|i| (i, screen(xf, q.corners[i]).distance(o)))
                .filter(|(_, d)| *d <= GRAB)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
        }
    }
    let Some(q) = view.camera.as_mut().filter(|q| q.page == page) else { return };
    let near = pointer.is_some_and(|p| q.corners.iter().any(|&c| screen(xf, c).distance(p) <= GRAB));
    if let Some(i) = q.grab {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(p) = pointer {
            let (x, y) = xf.screen_to_view(p);
            if let Some(c) = q.corners.get_mut(i) {
                *c = [x.clamp(0.0, xf.pw), y.clamp(0.0, xf.ph)];
            }
        }
        if resp.drag_stopped() {
            q.grab = None;
        }
    } else if near {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// The quadrilateral and its handles, with what will be cut away dimmed; or, while splitting a
/// spread, the line it will be split at.
pub(crate) fn paint(painter: &egui::Painter, xf: &PageXform, page: usize, view: &DocView) {
    if let Some(erase) = view.camera_erase.as_ref().filter(|e| e.page == page) {
        // The stroke so far, at the brush's width, until it's applied.
        let pts: Vec<Pos2> = erase.stroke.iter().map(|[u, v]| xf.norm_to_screen(*u, *v)).collect();
        let width = (erase.radius() * 2.0 * xf.rect.width()).max(2.0);
        if pts.len() == 1 {
            painter.circle_filled(pts[0], width / 2.0, Color32::from_white_alpha(200));
        } else if pts.len() > 1 {
            painter.add(Shape::line(pts, Stroke::new(width, Color32::from_white_alpha(200))));
        }
        return;
    }
    if let Some((split_page, fraction)) = view.camera_split {
        if split_page == page {
            let (top, bottom) = (xf.norm_to_screen(fraction, 0.0), xf.norm_to_screen(fraction, 1.0));
            painter.line_segment([top, bottom], Stroke::new(3.0, Color32::WHITE));
            painter.line_segment([top, bottom], Stroke::new(1.5, BLUE));
            let mid = top.lerp(bottom, 0.5);
            painter.circle(mid, 9.0, Color32::WHITE, Stroke::new(2.0, BLUE));
        }
        return;
    }
    let Some(q) = view.camera.filter(|q| q.page == page) else { return };
    let pts: Vec<Pos2> = q.corners.iter().map(|&c| screen(xf, c)).collect();
    // Dim the page outside the quadrilateral: a band from each edge out to the page's border.
    let r = xf.rect;
    let mut mesh = egui::Mesh::default();
    let dim = Color32::from_black_alpha(70);
    let outer = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
    // Pair each quad corner with the page corner nearest to it, in order around the page.
    let start = (0..4).min_by(|&a, &b| pts[0].distance_sq(outer[a]).total_cmp(&pts[0].distance_sq(outer[b]))).unwrap_or(0);
    for i in 0..4 {
        let (a, b) = (pts[i], pts[(i + 1) % 4]);
        let (oa, ob) = (outer[(start + i) % 4], outer[(start + i + 1) % 4]);
        let base = mesh.vertices.len() as u32;
        for p in [a, b, ob, oa] {
            mesh.colored_vertex(p, dim);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::closed_line(pts.clone(), Stroke::new(1.5, BLUE)));
    for p in pts {
        painter.circle(p, 6.0, Color32::WHITE, Stroke::new(2.0, BLUE));
    }
}

/// The floating bar at the top right of the document area (where the find bar goes, clear of a
/// centred page's corners): Enhance page and Cancel; once the page is enhanced, rotate it either
/// way, choose its look (original, colour, greyscale, black and white) and clean-up level,
/// Crop pages, Organize pages, or Done. The instructions are a notice (`ocr.enhance_camera`).
pub(crate) fn bar(ctx: &egui::Context, view: &mut DocView, area: Rect, t: &Tokens, allowed: bool) {
    egui::Area::new(egui::Id::new("camera-bar"))
        .order(egui::Order::Middle)
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(area.right_top() + vec2(-18.0, 12.0))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(t.card)
                .stroke(Stroke::new(1.0, t.border))
                .corner_radius(CornerRadius::same(10))
                .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if t.dark() { 90 } else { 30 }) })
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Eraser: brush size, and back to the bar.
                        if let Some(erase) = view.camera_erase.as_mut() {
                            ui.label(tl!("Brush over marks to remove them"));
                            ui.separator();
                            for (k, label) in [tl!("Small"), tl!("Medium"), tl!("Large")].into_iter().enumerate() {
                                if ui.add(egui::Button::selectable(erase.size == k, label)).clicked() {
                                    erase.size = k;
                                }
                            }
                            ui.separator();
                            if crate::widgets::pill_button(ui, tl!("Done erasing"), true).clicked() {
                                view.camera_erase = None;
                            }
                            return;
                        }
                        // Split pages: the line is on the page; split there, or don't.
                        if view.camera_split.is_some() {
                            ui.label(tl!("Drag the line onto the spine"));
                            if ui.add_enabled_ui(allowed, |ui| crate::widgets::pill_button(ui, tl!("Split here"), true)).inner.clicked() {
                                view.pending_action = Some(ViewAction::CameraSplit);
                            }
                            if crate::widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                                view.camera_split = None;
                            }
                            return;
                        }
                        // Enhance all pages under way: its progress, and Cancel.
                        if let Some(all) = &view.camera_all {
                            ui.spinner();
                            ui.label(crate::i18n::fmt(
                                tl!("Enhancing page {d} of {t}…"),
                                &[("d", &(all.finished + 1).min(all.total).to_string()), ("t", &all.total.to_string())],
                            ));
                            if crate::widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                                view.pending_action = Some(ViewAction::CameraAllCancel);
                            }
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
                            return;
                        }
                        // After Enhance page: put the page the right way up, trim it, or move on.
                        if let Some(done) = view.camera_done.as_mut() {
                            let mut again = false;
                            for (look, label) in [
                                (Look::Original, tl!("Original")),
                                (Look::Colour, tl!("Colour")),
                                (Look::Greyscale, tl!("Greyscale")),
                                (Look::BlackWhite, tl!("Black and white")),
                            ] {
                                if ui.add_enabled(allowed, egui::Button::selectable(done.look == look, label)).clicked() && done.look != look {
                                    done.look = look;
                                    again = true;
                                }
                            }
                            ui.separator();
                            ui.label(tl!("Clean-up"));
                            let slider = ui.add_enabled(
                                allowed && done.look != Look::Original,
                                egui::Slider::new(&mut done.level, 0..=crate::camera_ui::MAX_LEVEL).show_value(false),
                            );
                            // Enhance again once the slider is let go (or moved with the keys).
                            again |= slider.drag_stopped() || (slider.changed() && !slider.dragged());
                            if again && (done.look, done.level) != done.applied {
                                view.pending_action = Some(ViewAction::CameraLevel);
                            }
                            ui.separator();
                            let icon = |ui: &mut egui::Ui, name: &str, tip: &str| {
                                let r = ui.add_enabled(allowed, egui::Button::image(crate::icons::image(name, 18.0, t.text)).frame(false));
                                // Named for screen readers (and tests), not only on hover.
                                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, allowed, tip));
                                r.on_hover_text(tip).clicked()
                            };
                            if icon(ui, "rotate-ccw", tl!("Rotate counterclockwise")) {
                                view.pending_action = Some(ViewAction::CameraRotate { clockwise: false });
                            }
                            if icon(ui, "rotate-cw", tl!("Rotate clockwise")) {
                                view.pending_action = Some(ViewAction::CameraRotate { clockwise: true });
                            }
                            // Fingers, smudges and stains: brushed out with the paper's colour.
                            if crate::widgets::pill_button(ui, tl!("Eraser"), false)
                                .on_hover_text(tl!("Brush over fingers, smudges and stains to paint them out"))
                                .clicked()
                            {
                                view.camera_erase = Some(CameraErase { page: done.page, stroke: Vec::new(), size: 1 });
                            }
                            // A photographed book spread: two pages from one.
                            if crate::widgets::pill_button(ui, tl!("Split pages"), false)
                                .on_hover_text(tl!("Split a two-page spread into two pages"))
                                .clicked()
                            {
                                view.camera_split = Some((done.page, 0.5));
                            }
                            // The same look and clean-up for every other page of the document.
                            if crate::widgets::pill_button(ui, tl!("All pages"), false)
                                .on_hover_text(tl!("Straighten and clean up every other page the same way"))
                                .clicked()
                            {
                                view.pending_action = Some(ViewAction::CameraAll);
                            }
                            if crate::widgets::pill_button(ui, tl!("Crop pages"), false).clicked() {
                                view.pending_action = Some(ViewAction::CameraNext("page.crop"));
                            }
                            if crate::widgets::pill_button(ui, tl!("Organize pages"), false).clicked() {
                                view.pending_action = Some(ViewAction::CameraNext("page.organize"));
                            }
                            if crate::widgets::pill_button(ui, tl!("Done"), true).clicked() {
                                view.pending_action = Some(ViewAction::CancelCamera);
                            }
                            return;
                        }
                        let ready = allowed && view.camera.is_some();
                        if ui.add_enabled_ui(ready, |ui| crate::widgets::pill_button(ui, tl!("Enhance page"), true)).inner.clicked() {
                            view.pending_action = Some(ViewAction::EnhanceCamera);
                        }
                        if ui
                            .add_enabled_ui(allowed, |ui| crate::widgets::pill_button(ui, tl!("Enhance all pages"), false))
                            .inner
                            .on_hover_text(tl!("Find the edges on every page, then straighten and clean them all up"))
                            .clicked()
                        {
                            view.pending_action = Some(ViewAction::CameraAll);
                        }
                        if crate::widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                            view.pending_action = Some(ViewAction::CancelCamera);
                        }
                    });
                });
        });
}
