//! Design tokens and egui style. RevuCraft: charcoal chrome by default, one accent colour the
//! user picks (amber unless changed), pages stay white. Every custom widget reads `Tokens::get`.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ThemeKind {
    #[default]
    Light,
    Dark,
}

/// The saved user choice, independent of the light/dark colours currently displayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ThemePreference {
    System,
    Light,
    #[default]
    Dark,
}

/// The interface's accent colour (selections, active tools, primary buttons, links): the user's
/// choice, saved with the theme. Each is tuned separately for the dark and light themes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    #[default]
    Amber,
    Blue,
    Teal,
    Green,
    Red,
    Violet,
}

impl Accent {
    pub const ALL: [Accent; 6] = [Accent::Amber, Accent::Blue, Accent::Teal, Accent::Green, Accent::Red, Accent::Violet];

    /// The English name, translated where it is shown.
    pub fn label(self) -> &'static str {
        match self {
            Self::Amber => "Amber",
            Self::Blue => "Blue",
            Self::Teal => "Teal",
            Self::Green => "Green",
            Self::Red => "Red",
            Self::Violet => "Violet",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Amber => "amber",
            Self::Blue => "blue",
            Self::Teal => "teal",
            Self::Green => "green",
            Self::Red => "red",
            Self::Violet => "violet",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.code() == code)
    }

    /// (accent, accent_text, accent_soft, selected, on_accent) for a theme.
    fn colors(self, kind: ThemeKind) -> (Color32, Color32, Color32, Color32, Color32) {
        let rgb = |v: u32| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        let (a, text, soft, selected, on) = match (self, kind) {
            (Self::Amber, ThemeKind::Dark) => (0xF2A33A, 0xF5BA63, 0x3D2E17, 0x3A2C17, 0x1A1205),
            (Self::Amber, ThemeKind::Light) => (0xC77700, 0x8F5300, 0xFCEBD0, 0xFDF0DC, 0x1A1205),
            (Self::Blue, ThemeKind::Dark) => (0x4B8BF5, 0x8CB6FA, 0x243657, 0x233A63, 0xFFFFFF),
            (Self::Blue, ThemeKind::Light) => (0x1B63E0, 0x1755C4, 0xE3ECFD, 0xE6EEFD, 0xFFFFFF),
            (Self::Teal, ThemeKind::Dark) => (0x2BB3A3, 0x62D4C5, 0x173A37, 0x163A36, 0x06201D),
            (Self::Teal, ThemeKind::Light) => (0x0F7F73, 0x0B6B61, 0xDDF3F0, 0xE3F5F2, 0xFFFFFF),
            (Self::Green, ThemeKind::Dark) => (0x4CB772, 0x80D49C, 0x1B3A27, 0x1A3926, 0x07200F),
            (Self::Green, ThemeKind::Light) => (0x2E7D46, 0x256B3B, 0xE1F2E6, 0xE6F4EA, 0xFFFFFF),
            (Self::Red, ThemeKind::Dark) => (0xE5534B, 0xF28C86, 0x43201E, 0x46211F, 0xFFFFFF),
            (Self::Red, ThemeKind::Light) => (0xC62F28, 0xB0251F, 0xFBE3E1, 0xFCE8E6, 0xFFFFFF),
            (Self::Violet, ThemeKind::Dark) => (0x9B7BF0, 0xBCA6F7, 0x2E2552, 0x30275A, 0xFFFFFF),
            (Self::Violet, ThemeKind::Light) => (0x6B4BD8, 0x5A3BC4, 0xECE6FD, 0xEFEAFD, 0xFFFFFF),
        };
        (rgb(a), rgb(text), rgb(soft), rgb(selected), rgb(on))
    }
}

/// RevuCraft: the colour of the tool icons: the theme's accent, neutral grey, or a colour of their
/// own. The chosen tool's tile is the accent whichever it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum IconColor {
    #[default]
    Accent,
    Neutral,
    Own(Accent),
}

impl IconColor {
    /// `accent`, `neutral` or a colour's code (`blue`…), as settings and the control channel write it.
    pub fn code(self) -> &'static str {
        match self {
            Self::Accent => "accent",
            Self::Neutral => "neutral",
            Self::Own(a) => a.code(),
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "accent" => Some(Self::Accent),
            "neutral" => Some(Self::Neutral),
            other => Accent::from_code(other).map(Self::Own),
        }
    }

    /// The English name, translated where it is shown.
    pub fn label(self) -> &'static str {
        match self {
            Self::Accent => "Accent color",
            Self::Neutral => "Neutral",
            Self::Own(a) => a.label(),
        }
    }

    /// Every choice, for menus.
    pub fn all() -> Vec<Self> {
        let mut out = vec![Self::Accent, Self::Neutral];
        out.extend(Accent::ALL.into_iter().map(Self::Own));
        out
    }
}

/// Remember the tool icons' colour for this context; the next [`apply`] uses it.
pub fn set_icon_color(ctx: &egui::Context, c: IconColor) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("pdfcraft-icon-color"), c));
}

fn icon_color(ctx: &egui::Context) -> IconColor {
    ctx.data(|d| d.get_temp::<IconColor>(egui::Id::new("pdfcraft-icon-color"))).unwrap_or_default()
}

impl ThemePreference {
    /// The colours to draw. `system` is what the operating system says, `None` when it says
    /// nothing; `fallback` is then what the app last drew, so an unanswering desktop leaves the
    /// user's choice alone.
    pub fn resolve(self, system: Option<bool>, fallback: ThemeKind) -> ThemeKind {
        match self {
            Self::Light => ThemeKind::Light,
            Self::Dark => ThemeKind::Dark,
            Self::System => match system {
                Some(true) => ThemeKind::Dark,
                Some(false) => ThemeKind::Light,
                None => fallback,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub kind: ThemeKind,
    /// Title/tab strip.
    pub titlebar: Color32,
    /// Mode bar, panels.
    pub chrome: Color32,
    pub panel: Color32,
    /// Document area behind pages.
    pub pasteboard: Color32,
    pub card: Color32,
    pub border: Color32,
    pub divider: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub icon: Color32,
    /// RevuCraft: tool icons (ribbon, tool strips, rail, panel buttons): the accent, neutral or
    /// a colour of their own ([`IconColor`]).
    pub tool_icon: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub selected: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub accent_soft: Color32,
    /// Text and icons drawn on an `accent` fill (primary buttons, badges).
    pub on_accent: Color32,
    pub field: Color32,
    pub badge_new: Color32,
    pub page_shadow: Color32,
    pub radius: u8,
}

impl Tokens {
    /// The tokens for a theme with the default accent.
    pub fn for_kind(kind: ThemeKind) -> Self {
        Self::new(kind, Accent::default())
    }

    pub fn new(kind: ThemeKind, accent: Accent) -> Self {
        let mut t = Self::base(kind);
        let (a, text, soft, selected, on) = accent.colors(kind);
        t.accent = a;
        t.accent_text = text;
        t.accent_soft = soft;
        t.selected = selected;
        t.on_accent = on;
        t.badge_new = a;
        t.tool_icon = if kind == ThemeKind::Dark { a } else { text };
        t
    }

    /// The tokens with the tool icons in `c`.
    pub fn with_icon_color(mut self, c: IconColor) -> Self {
        self.tool_icon = match c {
            IconColor::Accent => self.tool_icon,
            IconColor::Neutral => self.icon,
            IconColor::Own(a) => {
                let (own, text, ..) = a.colors(self.kind);
                if self.kind == ThemeKind::Dark { own } else { text }
            }
        };
        self
    }

    fn base(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Light => Self {
                kind,
                titlebar: Color32::from_rgb(0xE9, 0xE9, 0xEB),
                chrome: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                panel: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                pasteboard: Color32::from_rgb(0xF1, 0xF1, 0xF3),
                card: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                border: Color32::from_rgb(0xDA, 0xDA, 0xDE),
                divider: Color32::from_rgb(0xE8, 0xE8, 0xEB),
                text: Color32::from_rgb(0x22, 0x22, 0x26),
                text_muted: Color32::from_rgb(0x5E, 0x5E, 0x66),
                text_faint: Color32::from_rgb(0x6B, 0x6B, 0x73),
                icon: Color32::from_rgb(0x44, 0x44, 0x4B),
                tool_icon: Color32::from_rgb(0x44, 0x44, 0x4B),
                hover: Color32::from_rgb(0xF0, 0xF0, 0xF3),
                pressed: Color32::from_rgb(0xE4, 0xE4, 0xE9),
                selected: Color32::from_rgb(0xE6, 0xEE, 0xFD),
                accent: Color32::from_rgb(0x1B, 0x63, 0xE0),
                accent_text: Color32::from_rgb(0x17, 0x55, 0xC4),
                accent_soft: Color32::from_rgb(0xE3, 0xEC, 0xFD),
                on_accent: Color32::WHITE,
                field: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                badge_new: Color32::from_rgb(0x1B, 0x63, 0xE0),
                page_shadow: Color32::from_black_alpha(34),
                radius: 6,
            },
            // Charcoal chrome; the pasteboard is lighter than the panels so white sheets and
            // dark linework both stand out.
            ThemeKind::Dark => Self {
                kind,
                titlebar: Color32::from_rgb(0x16, 0x18, 0x1B),
                chrome: Color32::from_rgb(0x1E, 0x20, 0x24),
                panel: Color32::from_rgb(0x1A, 0x1C, 0x20),
                pasteboard: Color32::from_rgb(0x2A, 0x2D, 0x32),
                card: Color32::from_rgb(0x24, 0x27, 0x2C),
                border: Color32::from_rgb(0x3A, 0x3F, 0x46),
                divider: Color32::from_rgb(0x2E, 0x32, 0x38),
                text: Color32::from_rgb(0xE6, 0xE8, 0xEC),
                text_muted: Color32::from_rgb(0xAE, 0xB4, 0xBD),
                text_faint: Color32::from_rgb(0x9A, 0xA0, 0xAA),
                icon: Color32::from_rgb(0xCD, 0xD1, 0xD8),
                tool_icon: Color32::from_rgb(0xCD, 0xD1, 0xD8),
                hover: Color32::from_rgb(0x2B, 0x2F, 0x35),
                pressed: Color32::from_rgb(0x35, 0x3A, 0x41),
                selected: Color32::from_rgb(0x23, 0x3A, 0x63),
                accent: Color32::from_rgb(0x4B, 0x8B, 0xF5),
                accent_text: Color32::from_rgb(0x8C, 0xB6, 0xFA),
                accent_soft: Color32::from_rgb(0x24, 0x36, 0x57),
                on_accent: Color32::WHITE,
                field: Color32::from_rgb(0x16, 0x18, 0x1B),
                badge_new: Color32::from_rgb(0x3D, 0x7D, 0xEE),
                page_shadow: Color32::from_black_alpha(120),
                radius: 6,
            },
        }
    }

    /// The current tokens (theme and accent, as last applied).
    pub fn get(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<Tokens>(egui::Id::new("pdfcraft-theme"))).unwrap_or_else(|| Self::for_kind(ThemeKind::Light))
    }

    pub fn dark(&self) -> bool {
        self.kind == ThemeKind::Dark
    }
}

pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}

/// Install the interface fonts with the CJK fallback order for the UI language (Chinese
/// first for Simplified Chinese, Japanese first otherwise). Call it when the language
/// changes; the new faces take effect next frame.
pub fn install_fonts_for(ctx: &egui::Context, prefer_hans: bool) {
    ctx.set_fonts(installed_font_definitions(prefer_hans));
}

/// The name of the installed face [`installed_font_definitions`] may add after the embedded ones.
pub const SYSTEM_FALLBACK: &str = "system-fallback";

/// What [`install_fonts_for`] installs: [`font_definitions_for`], then, on desktop, one face
/// already installed on this machine as the last fallback of every family. It only draws
/// characters no embedded face has (an Arabic file name in a build without craft-fonts);
/// `PDFCRAFT_SYSTEM_FONTS=0` leaves it out.
pub fn installed_font_definitions(prefer_hans: bool) -> FontDefinitions {
    #[cfg_attr(target_arch = "wasm32", expect(unused_mut))]
    let mut fonts = font_definitions_for(prefer_hans);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(data) = crate::system_fonts::fallback() {
        fonts.font_data.insert(SYSTEM_FALLBACK.to_owned(), data);
        for stack in fonts.families.values_mut() {
            stack.push(SYSTEM_FALLBACK.to_owned());
        }
    }
    fonts
}

/// The interface fonts: Inter (and JetBrains Mono for code) first, then egui's defaults, then
/// the CJK, Arabic and Telugu faces of the optional craft-fonts build input as the last fallback
/// in every family. Without craft-fonts there is no Japanese, Chinese, Arabic or Telugu face here.
pub fn font_definitions() -> FontDefinitions {
    font_definitions_for(false)
}

/// [`font_definitions`] with the CJK fallback order for the UI language. Simplified Chinese
/// must come first in Chinese mode: otherwise shared characters render in the Japanese face
/// while Simplified-only characters (e.g. U+6B22 欢) fall through to the Chinese face, and
/// the mixed vertical metrics sink them below the line.
pub fn font_definitions_for(prefer_hans: bool) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(&mut fonts, "Inter", include_bytes!("../../../assets/fonts/Inter-Regular.ttf"));
    add(&mut fonts, "Inter-Medium", include_bytes!("../../../assets/fonts/Inter-Medium.ttf"));
    add(&mut fonts, "Inter-SemiBold", include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"));
    add(&mut fonts, "JetBrainsMono", include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"));
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "Inter".to_owned());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "JetBrainsMono".to_owned());
    // The same static bytes pdfcraft-fonts uses for Japanese/Chinese text in PDFs: one copy, not two.
    for face in pdfcraft_fonts::ui_cjk_fonts(prefer_hans) {
        let name = face.name();
        // Japanese and Chinese faces have distinct family names, so no collision here.
        if !fonts.font_data.contains_key(&name) {
            add(&mut fonts, &name, face.bytes);
        }
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push(name.clone());
        }
    }
    // Arabic-script faces (file names, document titles) and Telugu faces (the Telugu interface,
    // file names) after the CJK ones; the ranges don't overlap.
    for face in pdfcraft_fonts::ui_arabic_fonts().into_iter().chain(pdfcraft_fonts::ui_telugu_fonts()) {
        let name = face.name();
        if !fonts.font_data.contains_key(&name) {
            add(&mut fonts, &name, face.bytes);
        }
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let stack = fonts.families.entry(family).or_default();
            if !stack.contains(&name) {
                stack.push(name.clone());
            }
        }
    }
    let fallback: Vec<String> = fonts.families[&FontFamily::Proportional].clone();
    for (fam, primary) in [("medium", "Inter-Medium"), ("semibold", "Inter-SemiBold")] {
        let mut stack = vec![primary.to_owned()];
        stack.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(fam.into()), stack);
    }
    fonts
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

/// The accent [`apply`] uses, as last set with [`set_accent`].
pub fn accent(ctx: &egui::Context) -> Accent {
    ctx.data(|d| d.get_temp::<Accent>(egui::Id::new("pdfcraft-accent"))).unwrap_or_default()
}

/// Remember `accent` for this context; the next [`apply`] uses it.
pub fn set_accent(ctx: &egui::Context, accent: Accent) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("pdfcraft-accent"), accent));
}

pub fn apply(ctx: &egui::Context, kind: ThemeKind) {
    // egui must use the same theme for popup/menu styles as our custom chrome.
    ctx.set_theme(if kind == ThemeKind::Dark { egui::Theme::Dark } else { egui::Theme::Light });
    let t = Tokens::new(kind, accent(ctx)).with_icon_color(icon_color(ctx));
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("pdfcraft-theme"), t));
    let mut v = if t.dark() { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = t.panel;
    v.window_fill = t.card;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.hover;
    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent_text;
    v.override_text_color = Some(t.text);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if t.dark() { 110 } else { 38 }) };
    v.popup_shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if t.dark() { 90 } else { 30 }) };
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(t.radius);
        w.fg_stroke.color = t.text;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.divider);
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = t.field;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_fill = t.hover;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.weak_bg_fill = t.pressed;
    v.widgets.active.bg_fill = t.pressed;
    v.widgets.open.weak_bg_fill = t.hover;
    // Crisper text at 100–150 % scaling (#76): glyphs sit on whole pixels instead of being
    // rendered at quarter-pixel offsets, which egui notes blurs them. In the light theme, a mild
    // gamma darkens the antialiased edges of dark text (egui's default is linear, which reads thin
    // and grey next to the system's text); the dark theme keeps egui's own curve. At 200 % both
    // make little difference.
    v.text_options.subpixel_binning = false;
    if !t.dark() {
        v.text_options.color_transfer_function = egui::epaint::FontColorTransferFunction::Gamma(0.75);
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.scroll.bar_width = 8.0;
        s.spacing.scroll.floating = true;
        s.text_styles.insert(egui::TextStyle::Body, regular(13.0));
        s.text_styles.insert(egui::TextStyle::Button, regular(13.0));
        s.text_styles.insert(egui::TextStyle::Small, regular(11.0));
        s.text_styles.insert(egui::TextStyle::Heading, semibold(17.0));
        s.interaction.tooltip_delay = 0.35;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Use system setting" draws what the operating system says, dark or light.
    #[test]
    fn following_the_system_draws_what_the_system_says() {
        assert_eq!(ThemePreference::System.resolve(Some(true), ThemeKind::Light), ThemeKind::Dark);
        assert_eq!(ThemePreference::System.resolve(Some(false), ThemeKind::Dark), ThemeKind::Light);
    }

    /// An operating system that says nothing leaves the last choice standing, and an explicit
    /// Light or Dark is never overridden by what the system says.
    #[test]
    fn an_answerless_system_leaves_the_choice_and_an_explicit_one_stands() {
        assert_eq!(ThemePreference::System.resolve(None, ThemeKind::Light), ThemeKind::Light);
        assert_eq!(ThemePreference::System.resolve(None, ThemeKind::Dark), ThemeKind::Dark);
        assert_eq!(ThemePreference::Light.resolve(Some(true), ThemeKind::Dark), ThemeKind::Light);
        assert_eq!(ThemePreference::Dark.resolve(Some(false), ThemeKind::Light), ThemeKind::Dark);
    }

    /// WCAG 2 contrast ratio between two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let lin = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
        };
        let (x, y) = (lum(a) + 0.05, lum(b) + 0.05);
        x.max(y) / x.min(y)
    }

    #[test]
    fn text_is_readable_on_every_surface() {
        // #76: the faintest text (hints, zoom level, captions) was 3.3:1 in the light theme.
        for kind in [ThemeKind::Light, ThemeKind::Dark] {
            let t = Tokens::for_kind(kind);
            for (name, fg) in [("text", t.text), ("text_muted", t.text_muted), ("text_faint", t.text_faint)] {
                for bg in [t.chrome, t.panel, t.card, t.pasteboard] {
                    let r = contrast(fg, bg);
                    assert!(r >= 4.5, "{kind:?} {name} on {bg:?}: {r:.2}:1, WCAG AA needs 4.5:1");
                }
            }
        }
    }

    /// Every accent stays readable in both themes: links and accent text on the panels, and
    /// button text on an accent fill.
    #[test]
    fn every_accent_is_readable_in_both_themes() {
        for kind in [ThemeKind::Light, ThemeKind::Dark] {
            for accent in Accent::ALL {
                let t = Tokens::new(kind, accent);
                for bg in [t.chrome, t.panel, t.card] {
                    let r = contrast(t.accent_text, bg);
                    assert!(r >= 4.5, "{kind:?} {accent:?} accent text on {bg:?}: {r:.2}:1");
                }
                let r = contrast(t.on_accent, t.accent);
                assert!(r >= 3.0, "{kind:?} {accent:?} text on the accent fill: {r:.2}:1");
                assert!(contrast(t.text, t.selected) >= 4.5, "{kind:?} {accent:?} text on a selected row");
            }
        }
    }

    #[test]
    fn accents_round_trip_by_code() {
        for a in Accent::ALL {
            assert_eq!(Accent::from_code(a.code()), Some(a));
        }
        assert_eq!(Accent::default(), Accent::Amber);
        assert_eq!(ThemePreference::default(), ThemePreference::Dark);
    }
}
