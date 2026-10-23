//! Design tokens and egui style. RevuCraft: charcoal chrome by default, one accent colour the
//! user picks (amber unless changed), pages stay white. Every custom widget reads `Tokens::get`.

use std::sync::Arc;

use egui::epaint::text::{FontTweak, HintingTarget, SmoothHinting};
use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Rect, Response, Stroke, Vec2, Visuals};

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
    /// nothing; `fallback` is supplied by the caller. PdfCraft uses Light for an unknown desktop.
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

/// Hover and press amounts for chrome controls. The wash eases in; holding the pointer
/// darkens it and drops the content one pixel, so a click reads as a key settling.
#[derive(Clone, Copy, Debug)]
pub struct Press {
    pub hover: f32,
    pub down: f32,
}

impl Press {
    pub fn track(ui: &egui::Ui, resp: &Response) -> Self {
        let hot = resp.hovered() || resp.is_pointer_button_down_on();
        let ease = egui::emath::easing::quadratic_out;
        // With animation effects off the wash and the press are simply there.
        let (hover, down) = if crate::motion::animated(ui.ctx()) { (0.12, 0.06) } else { (0.0, 0.0) };
        Self {
            hover: ui.ctx().animate_bool_with_time_and_easing(resp.id.with("chrome-hover"), hot, hover, ease),
            down: ui.ctx().animate_bool_with_time_and_easing(resp.id.with("chrome-down"), resp.is_pointer_button_down_on(), down, ease),
        }
    }

    pub fn fill(self, rest: Color32, hover: Color32, pressed: Color32) -> Color32 {
        rest.lerp_to_gamma(hover, self.hover).lerp_to_gamma(pressed, self.down)
    }

    /// One pixel down at a full press.
    pub fn offset(self) -> Vec2 {
        Vec2::new(0.0, self.down)
    }

    /// The colour [`Self::wash`] paints.
    pub fn wash_color(self, t: &Tokens, selected: bool) -> Color32 {
        if selected {
            self.fill(t.accent_soft, t.accent_soft.lerp_to_gamma(t.accent, 0.16), t.selected)
        } else {
            self.fill(Color32::TRANSPARENT, t.hover, t.pressed)
        }
    }

    /// Soft wash behind a control. `selected` keeps the accent fill.
    pub fn wash(self, ui: &egui::Ui, rect: Rect, radius: impl Into<CornerRadius>, selected: bool) {
        let fill = self.wash_color(&Tokens::get(ui.ctx()), selected);
        if fill.a() > 0 {
            ui.painter().rect_filled(rect, radius, fill);
        }
    }
}

/// Pointing hand, the cursor for a control that clicks.
pub fn hand(resp: Response) -> Response {
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn install_fonts(ctx: &egui::Context) {
    install_fonts_for(ctx, CjkPreference::Default);
}

/// The interface language's claim on the CJK fallback faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CjkPreference {
    /// Japanese faces first (every non-Chinese language).
    #[default]
    Default,
    SimplifiedChinese,
    TraditionalChinese,
}

impl CjkPreference {
    /// The preference for a UI language code (`zh-hans`, `zh-hant`, anything else).
    pub fn for_language(code: &str) -> Self {
        match code {
            "zh-hans" => Self::SimplifiedChinese,
            "zh-hant" => Self::TraditionalChinese,
            _ => Self::Default,
        }
    }
}

/// Install the interface fonts with the CJK fallback order for the UI language (Chinese
/// first for Chinese, Japanese first otherwise). Call it when the language changes; the new
/// faces take effect next frame.
pub fn install_fonts_for(ctx: &egui::Context, cjk: CjkPreference) {
    ctx.set_fonts(installed_font_definitions(cjk));
}

/// The name of the installed face [`installed_font_definitions`] may add after the embedded ones.
pub const SYSTEM_FALLBACK: &str = "system-fallback";
/// The name of the installed Devanagari (Hindi) face [`installed_font_definitions`] may add.
pub const SYSTEM_DEVANAGARI_FALLBACK: &str = "system-devanagari-fallback";
/// The name of the installed Telugu face [`installed_font_definitions`] may add.
pub const SYSTEM_TELUGU_FALLBACK: &str = "system-telugu-fallback";
/// The name of the installed Sinhala face [`installed_font_definitions`] may add.
pub const SYSTEM_SINHALA_FALLBACK: &str = "system-sinhala-fallback";
/// The name of the installed Tamil face [`installed_font_definitions`] may add.
pub const SYSTEM_TAMIL_FALLBACK: &str = "system-tamil-fallback";
/// The name of the installed Hebrew face [`installed_font_definitions`] may add.
pub const SYSTEM_HEBREW_FALLBACK: &str = "system-hebrew-fallback";
/// The name of the installed Hangul (Korean) face [`installed_font_definitions`] may add.
pub const SYSTEM_HANGUL_FALLBACK: &str = "system-hangul-fallback";

/// What [`install_fonts_for`] installs: [`font_definitions_for`], then, on desktop, one face
/// already installed on this machine. It only draws characters no embedded face has.
///
/// - In a Chinese interface built without a `Hans` craft-fonts face (every release so far,
///   #826), it is a Chinese system face (Microsoft YaHei, PingFang, …), placed before the
///   Japanese craft-fonts faces: they lack hundreds of the catalog's characters (欢, 导, 签, …),
///   and one line drawn from two faces with different vertical metrics would not sit on one
///   baseline.
/// - Otherwise it is the last fallback of every family (an Arabic file name in a build without
///   craft-fonts).
///
/// Installed Devanagari, Telugu, Sinhala, Tamil, Hebrew and Hangul faces, when there are, end
/// every family just before that one, so Hindi, Telugu (#1531), Sinhala and Tamil (#1688) and
/// Korean text, such as a file name or a tab title, and Hebrew file names (#586) draw in a build
/// without such faces.
///
/// `PDFCRAFT_SYSTEM_FONTS=0` leaves them out.
pub fn installed_font_definitions(cjk: CjkPreference) -> FontDefinitions {
    #[cfg_attr(target_arch = "wasm32", expect(unused_mut))]
    let mut fonts = font_definitions_for(cjk == CjkPreference::SimplifiedChinese);
    #[cfg(not(target_arch = "wasm32"))]
    {
        use crate::system_fonts::{Need, fallback};
        // Before the general installed face, which stays the last fallback outside Chinese.
        for (name, data) in [
            (SYSTEM_DEVANAGARI_FALLBACK, crate::system_fonts::devanagari_fallback()),
            (SYSTEM_TELUGU_FALLBACK, crate::system_fonts::telugu_fallback()),
            (SYSTEM_SINHALA_FALLBACK, crate::system_fonts::sinhala_fallback()),
            (SYSTEM_TAMIL_FALLBACK, crate::system_fonts::tamil_fallback()),
            (SYSTEM_HEBREW_FALLBACK, crate::system_fonts::hebrew_fallback()),
            (SYSTEM_HANGUL_FALLBACK, crate::system_fonts::hangul_fallback()),
        ] {
            let Some(data) = data else { continue };
            fonts.font_data.insert(name.to_owned(), data);
            for stack in fonts.families.values_mut() {
                stack.push(name.to_owned());
            }
        }
        let need = match cjk {
            _ if !pdfcraft_fonts::ui_chinese_fonts().is_empty() => Need::Other,
            CjkPreference::SimplifiedChinese => Need::Hans,
            CjkPreference::TraditionalChinese => Need::Hant,
            CjkPreference::Default => Need::Other,
        };
        if let Some(data) = fallback(need) {
            fonts.font_data.insert(SYSTEM_FALLBACK.to_owned(), data);
            let japanese: Vec<String> = pdfcraft_fonts::ui_japanese_fonts().iter().map(|f| f.name()).collect();
            for stack in fonts.families.values_mut() {
                let at = match need {
                    Need::Other => None,
                    Need::Hans | Need::Hant => stack.iter().position(|n| japanese.contains(n)),
                };
                stack.insert(at.unwrap_or(stack.len()), SYSTEM_FALLBACK.to_owned());
            }
        }
    }
    fonts
}

/// The interface fonts: Inter (and JetBrains Mono for code) first, then egui's defaults, then
/// the CJK, Arabic, Telugu, Hebrew and Thai faces of the optional craft-fonts build input as the
/// last fallback in every family. Without craft-fonts there is no Japanese, Chinese, Arabic,
/// Telugu, Hebrew or Thai face here.
pub fn font_definitions() -> FontDefinitions {
    font_definitions_for(false)
}

/// [`font_definitions`] with the CJK fallback order for the UI language. Simplified Chinese
/// must come first in Chinese mode: otherwise shared characters render in the Japanese face
/// while Simplified-only characters (e.g. U+6B22 欢) fall through to the Chinese face, and
/// the mixed vertical metrics sink them below the line.
pub fn font_definitions_for(prefer_hans: bool) -> FontDefinitions {
    font_definitions_for_platform(prefer_hans, current_font_policy_platform())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FontPolicyPlatform {
    Windows,
    Other,
}

fn current_font_policy_platform() -> FontPolicyPlatform {
    if cfg!(target_os = "windows") { FontPolicyPlatform::Windows } else { FontPolicyPlatform::Other }
}

fn primary_font_tweak(platform: FontPolicyPlatform, name: &str) -> FontTweak {
    let mut tweak = FontTweak::default();
    if platform == FontPolicyPlatform::Windows && name == "JetBrainsMono" {
        tweak.hinting_target = HintingTarget::Smooth(SmoothHinting { symmetric_rendering: false, ..SmoothHinting::default() });
    }
    tweak
}

/// A face's vertical metrics in em, as egui reads them: skrifa's ascent, descent (negative) and
/// line gap at the default location. `None` when the face doesn't parse.
fn vertical_metrics(bytes: &[u8], index: u32) -> Option<[f32; 3]> {
    use skrifa::MetadataProvider as _;
    let font = skrifa::FontRef::from_index(bytes, index).ok()?;
    let m = font.metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
    let em = f32::from(m.units_per_em);
    let metrics = [m.ascent / em, m.descent / em, m.leading / em];
    (em > 0.0 && metrics.iter().all(|v| v.is_finite())).then_some(metrics)
}

/// How far below the primary face's baseline egui draws a fallback face's, in em of the font size
/// (negative: above). egui centres the fallback's line box (ascent − descent + gap) on the
/// primary's rather than lining up their baselines, so the baseline moves by half the difference
/// of ascent + descent, less half the difference of the line gaps (epaint `text_layout`).
fn baseline_drop(primary: [f32; 3], fallback: [f32; 3]) -> f32 {
    let [pa, pd, pg] = primary;
    let [a, d, g] = fallback;
    ((a + d) - (pa + pd) + pg - g) / 2.0
}

/// `data` with its glyphs moved onto Inter's baseline (#1676). Without this, Chinese next to Latin
/// in one label sits higher or lower than the Latin by a few percent of the font size, depending on
/// the installed Chinese face (PingFang, Microsoft JhengHei, SimSun…). Inter is the first face of
/// every proportional family; the monospace family's JetBrains Mono differs from it by 0.004 em,
/// well under a pixel. A face that doesn't parse is left as it is (egui will skip it anyway).
pub(crate) fn on_interface_baseline(mut data: FontData) -> FontData {
    let primary = vertical_metrics(pdfcraft_fonts::INTER_REGULAR, 0);
    if let (Some(primary), Some(fallback)) = (primary, vertical_metrics(&data.font, data.index)) {
        data.tweak.y_offset_factor = -baseline_drop(primary, fallback);
    }
    data
}

fn font_definitions_for_platform(prefer_hans: bool, platform: FontPolicyPlatform) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        let data = FontData::from_static(bytes);
        let data = if ["Inter", "Inter-Medium", "Inter-SemiBold", "JetBrainsMono"].contains(&name) {
            data.tweak(primary_font_tweak(platform, name))
        } else {
            on_interface_baseline(data)
        };
        fonts.font_data.insert(name.to_owned(), Arc::new(data));
    };
    // Shared with typed signatures' fallback glyphs, so the font is embedded once.
    add(&mut fonts, "Inter", pdfcraft_fonts::INTER_REGULAR);
    add(&mut fonts, "Inter-Medium", include_bytes!("../../../assets/fonts/Inter-Medium.ttf"));
    add(&mut fonts, "Inter-SemiBold", include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"));
    add(&mut fonts, "JetBrainsMono", include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"));
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "Inter".to_owned());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "JetBrainsMono".to_owned());
    // Keep missing Latin/Cyrillic letters in the app's own faces (for example Bulgarian ѝ,
    // absent from JetBrains Mono), before egui's defaults or optional system/CJK fallbacks.
    fonts.families.entry(FontFamily::Monospace).or_default().insert(1, "Inter".to_owned());
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
    // Arabic-script faces (file names, document titles), Telugu faces (the Telugu interface,
    // file names), Hebrew faces (file names, text being edited), Thai faces and Devanagari faces
    // (the Hindi interface) after the CJK ones; the ranges don't overlap.
    for face in pdfcraft_fonts::ui_arabic_fonts()
        .into_iter()
        .chain(pdfcraft_fonts::ui_telugu_fonts())
        .chain(pdfcraft_fonts::ui_hebrew_fonts())
        .chain(pdfcraft_fonts::ui_thai_fonts())
        .chain(pdfcraft_fonts::ui_devanagari_fonts())
    {
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
    // Keep the native window inherited. A concrete egui theme pins macOS's
    // NSWindow appearance and prevents winit from delivering ThemeChanged.
    // Our saved choice owns the palette; both egui branches receive it below.
    ctx.set_theme(egui::ThemePreference::System);
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
    // Stock buttons (menus, dialogs) use the same pointing hand as the custom chrome.
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);
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
        s.spacing.scroll.floating_width = 3.0;
        s.spacing.scroll.foreground_color = true;
        s.text_styles.insert(egui::TextStyle::Body, regular(13.0));
        s.text_styles.insert(egui::TextStyle::Button, regular(13.0));
        s.text_styles.insert(egui::TextStyle::Small, regular(11.0));
        s.text_styles.insert(egui::TextStyle::Heading, semibold(17.0));
        s.interaction.tooltip_delay = 0.35;
    });
    let style = ctx.global_style();
    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_style_of(egui::Theme::Dark, style);
}

/// WCAG 2 contrast ratio between two opaque colours.
pub fn contrast(a: Color32, b: Color32) -> f32 {
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

/// `color` as text on `background`: itself when it reads (a WCAG 2 contrast of at least 3:1, the
/// level for large or bold text), otherwise moved towards `text` until it does. A document's own
/// colours (a bookmark's `/C`) were chosen for a white panel, and dark blue on a dark theme would
/// be lost.
pub fn readable(color: Color32, background: Color32, text: Color32) -> Color32 {
    (0..=4).map(|step| color.lerp_to_gamma(text, step as f32 / 4.0)).find(|c| contrast(*c, background) >= 3.0).unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #3193: a bookmark's own colour is kept where it reads on the panel, and moved towards the
    /// theme's text colour where it doesn't (dark blue on the dark panel), never past it.
    #[test]
    fn a_document_colour_is_kept_where_it_reads() {
        let (light, dark) = (Tokens::for_kind(ThemeKind::Light), Tokens::for_kind(ThemeKind::Dark));
        let red = Color32::from_rgb(255, 0, 0);
        assert_eq!(readable(red, light.panel, light.text), red, "red reads on white");
        let navy = Color32::from_rgb(0, 0, 128);
        assert_eq!(readable(navy, light.panel, light.text), navy);
        let lifted = readable(navy, dark.panel, dark.text);
        assert_ne!(lifted, navy, "navy is lost on the dark panel");
        assert!(contrast(lifted, dark.panel) >= 3.0, "{lifted:?}");
        assert_ne!(lifted, dark.text, "only as far as it needs");
        // Even the panel's own colour comes out readable.
        assert!(contrast(readable(dark.panel, dark.panel, dark.text), dark.panel) >= 3.0);
        assert!(contrast(readable(light.panel, light.panel, light.text), light.panel) >= 3.0);
    }
    use egui::epaint::text::{Fonts, TextOptions};

    const HINTING_CORPUS: [&str; 4] = ["Il1MWmw", "Hamburgefonts", "0123456789 .,;:!?()[]{}+-=/\\", "Café Ångström déjà-vu Ωμέγα Ελληνικά Журнал"];
    const HINTING_SIZES: [f32; 6] = [10.0, 11.0, 12.0, 13.0, 14.0, 16.0];
    const HINTING_PPPS: [f32; 4] = [1.0, 1.25, 1.5, 2.0];

    fn primary_families() -> [(FontFamily, &'static str); 4] {
        [
            (FontFamily::Proportional, "Inter"),
            (FontFamily::Name("medium".into()), "Inter-Medium"),
            (FontFamily::Name("semibold".into()), "Inter-SemiBold"),
            (FontFamily::Monospace, "JetBrainsMono"),
        ]
    }

    fn raster_edge_and_ink(fonts: &mut Fonts, id: FontId, text: &str, ppp: f32) -> (u64, u64) {
        let galley = {
            let mut view = fonts.with_pixels_per_point(ppp);
            view.layout_no_wrap(text.to_owned(), id, Color32::BLACK).as_ref().clone()
        };
        let image = fonts.image();
        let mut edge = 0u64;
        let mut ink = 0u64;
        for glyph in galley.rows.iter().flat_map(|row| &row.glyphs) {
            let [x0, y0] = glyph.uv_rect.min.map(usize::from);
            let [x1, y1] = glyph.uv_rect.max.map(usize::from);
            for y in y0..y1 {
                for x in x0..x1 {
                    let alpha = image.pixels[y * image.size[0] + x].a();
                    ink += u64::from(alpha);
                    if x > x0 {
                        let left = image.pixels[y * image.size[0] + x - 1].a();
                        edge += u64::from(alpha.abs_diff(left));
                    }
                }
            }
        }
        (edge, ink)
    }

    fn mono_raster_measure(fonts: &mut Fonts, ppp: f32) -> (u64, u64) {
        let mut edge = 0;
        let mut ink = 0;
        for size in HINTING_SIZES {
            for text in HINTING_CORPUS {
                let (glyph_edge, glyph_ink) = raster_edge_and_ink(fonts, FontId::monospace(size), text, ppp);
                edge += glyph_edge;
                ink += glyph_ink;
            }
        }
        (edge, ink)
    }

    fn layout_signature(galley: &egui::Galley) -> Vec<u32> {
        let mut signature = vec![
            galley.rect.min.x.to_bits(),
            galley.rect.min.y.to_bits(),
            galley.rect.max.x.to_bits(),
            galley.rect.max.y.to_bits(),
            u32::from(galley.elided),
            galley.rows.len() as u32,
        ];
        for row in &galley.rows {
            signature.extend([
                u32::from(row.ends_with_newline),
                row.pos.x.to_bits(),
                row.pos.y.to_bits(),
                row.size.x.to_bits(),
                row.size.y.to_bits(),
                row.glyphs.len() as u32,
            ]);
            for glyph in &row.glyphs {
                signature.extend([
                    glyph.chr as u32,
                    glyph.pos.x.to_bits(),
                    glyph.pos.y.to_bits(),
                    glyph.advance_width.to_bits(),
                    glyph.line_height.to_bits(),
                    glyph.font_ascent.to_bits(),
                    glyph.font_height.to_bits(),
                    glyph.font_face_ascent.to_bits(),
                    glyph.font_face_height.to_bits(),
                ]);
            }
        }
        signature
    }

    fn galley(fonts: &mut Fonts, family: FontFamily, size: f32, ppp: f32, text: &str) -> egui::Galley {
        let mut view = fonts.with_pixels_per_point(ppp);
        view.layout_no_wrap(text.to_owned(), FontId::new(size, family), Color32::BLACK).as_ref().clone()
    }

    fn hinting_test_fonts(platform: FontPolicyPlatform) -> Fonts {
        Fonts::new(
            TextOptions {
                subpixel_binning: false,
                color_transfer_function: egui::epaint::FontColorTransferFunction::Gamma(0.75),
                ..TextOptions::default()
            },
            font_definitions_for_platform(false, platform),
        )
    }

    /// The baseline of each glyph of `text` laid out by egui at `size` points, in points from the
    /// top of the row: its position plus its face's `y_offset_factor`, as epaint draws it.
    fn baselines(fallback: FontData, size: f32, text: &str) -> Vec<f32> {
        let factor = fallback.tweak.y_offset_factor;
        let mut defs = FontDefinitions::empty();
        defs.font_data.insert("Inter".into(), Arc::new(FontData::from_static(pdfcraft_fonts::INTER_REGULAR)));
        defs.font_data.insert("fallback".into(), Arc::new(fallback));
        defs.families.insert(FontFamily::Proportional, vec!["Inter".into(), "fallback".into()]);
        let mut fonts = Fonts::new(TextOptions::default(), defs);
        let g = galley(&mut fonts, FontFamily::Proportional, size, 1.0, text);
        let inter_has = |c: char| c.is_ascii();
        g.rows[0].glyphs.iter().map(|gl| gl.pos.y + if inter_has(gl.chr) { 0.0 } else { size * factor }).collect()
    }

    #[test]
    fn fallback_faces_share_the_interface_baseline() {
        // Dancing Script's metrics differ from Inter's the way a Chinese face's do (by 0.044 em),
        // and it has letters Inter doesn't (Ǆ), which egui then takes from it.
        let dancing: &'static [u8] = include_bytes!("../../../assets/fonts/DancingScript.ttf");
        let size = 100.0;
        // As egui lays it out by itself, Ǆ sits ~4 pt off H's baseline: the bug (#1676).
        let raw = baselines(FontData::from_static(dancing), size, "HǄ");
        assert!((raw[1] - raw[0]).abs() > 3.0, "the test fonts must differ: {raw:?}");
        let drop = baseline_drop(vertical_metrics(pdfcraft_fonts::INTER_REGULAR, 0).unwrap(), vertical_metrics(dancing, 0).unwrap());
        assert!((raw[1] - raw[0] - size * drop).abs() <= 1.0, "baseline_drop predicts egui: {raw:?} vs {drop}");
        // On the interface baseline, to the pixel.
        let aligned = baselines(on_interface_baseline(FontData::from_static(dancing)), size, "HǄ");
        assert!((aligned[1] - aligned[0]).abs() <= 1.0, "{aligned:?}");
        // A face that doesn't parse is left alone rather than guessed at.
        assert_eq!(on_interface_baseline(FontData::from_static(b"not a font")).tweak, FontTweak::default());
        assert_eq!(vertical_metrics(b"", 0), None);
    }

    #[test]
    fn windows_hinting_changes_only_the_jetbrains_symmetric_rendering_field() {
        let baseline = font_definitions_for_platform(false, FontPolicyPlatform::Other);
        let windows = font_definitions_for_platform(false, FontPolicyPlatform::Windows);

        assert_eq!(baseline.families, windows.families);
        assert_eq!(baseline.font_data.keys().collect::<Vec<_>>(), windows.font_data.keys().collect::<Vec<_>>());
        for (family, name) in primary_families() {
            let baseline_data = &baseline.font_data[name];
            let windows_data = &windows.font_data[name];
            assert_eq!(baseline_data.tweak, FontTweak::default(), "baseline {name}");
            if name == "JetBrainsMono" {
                assert_eq!(
                    windows_data.tweak,
                    FontTweak {
                        hinting_target: HintingTarget::Smooth(SmoothHinting { symmetric_rendering: false, ..SmoothHinting::default() }),
                        ..FontTweak::default()
                    },
                    "Windows {name}"
                );
            } else {
                assert_eq!(windows_data.tweak, baseline_data.tweak, "Windows {name}");
            }
            assert_eq!(primary_font_tweak(FontPolicyPlatform::Other, name), baseline_data.tweak, "non-Windows policy for {family:?}");
        }
        for (name, data) in &baseline.font_data {
            if !["Inter", "Inter-Medium", "Inter-SemiBold", "JetBrainsMono"].contains(&name.as_str()) {
                assert_eq!(windows.font_data[name].tweak, data.tweak, "Windows fallback {name}");
            }
        }
        assert_eq!(
            font_definitions_for(false).font_data["JetBrainsMono"].tweak,
            font_definitions_for_platform(false, current_font_policy_platform()).font_data["JetBrainsMono"].tweak
        );
    }

    #[test]
    #[ignore = "measurement probe: asserts exact egui rasterisation deltas, which change with egui upgrades; run with --ignored when retuning the hinting"]
    fn windows_hinting_improves_low_dpi_mono_raster_without_changing_layout_or_coverage() {
        let mut baseline = hinting_test_fonts(FontPolicyPlatform::Other);
        let mut windows = hinting_test_fonts(FontPolicyPlatform::Windows);

        for ppp in HINTING_PPPS {
            let (baseline_edge, baseline_ink) = mono_raster_measure(&mut baseline, ppp);
            let (windows_edge, windows_ink) = mono_raster_measure(&mut windows, ppp);
            // Compare horizontal alpha-edge energy per ink alpha; this is a deterministic
            // vertical-stem sharpness proxy, not an absolute typography quality score.
            let baseline_sharpness = baseline_edge as f64 / baseline_ink as f64;
            let windows_sharpness = windows_edge as f64 / windows_ink as f64;
            let sharpness_change = windows_sharpness / baseline_sharpness - 1.0;
            let weight_change = windows_ink as f64 / baseline_ink as f64 - 1.0;
            if ppp < 2.0 {
                assert!(sharpness_change > 0.002, "{ppp} ppp sharpness change: {sharpness_change:.2}%", sharpness_change = sharpness_change * 100.0);
            } else {
                assert!(sharpness_change >= -0.01, "{ppp} ppp sharpness change: {sharpness_change:.2}%", sharpness_change = sharpness_change * 100.0);
            }
            assert!(weight_change.abs() < 0.05, "{ppp} ppp ink change: {weight_change:.2}%", weight_change = weight_change * 100.0);
        }

        for ppp in HINTING_PPPS {
            for size in HINTING_SIZES {
                for (family, _) in primary_families() {
                    for text in HINTING_CORPUS {
                        assert_eq!(
                            baseline.has_glyphs(&FontId::new(size, family.clone()), text),
                            windows.has_glyphs(&FontId::new(size, family.clone()), text),
                            "{family:?} {text:?}"
                        );
                        assert_eq!(
                            layout_signature(&galley(&mut baseline, family.clone(), size, ppp, text)),
                            layout_signature(&galley(&mut windows, family.clone(), size, ppp, text)),
                            "{family:?}, {size} pt, {ppp} ppp, {text:?}"
                        );
                    }
                }
            }
        }
    }

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

    #[test]
    fn press_darkens_then_sinks() {
        let rest = Color32::from_rgb(0xFF, 0xFF, 0xFF);
        let hover = Color32::from_rgb(0xF0, 0xF0, 0xF3);
        let pressed = Color32::from_rgb(0xE4, 0xE4, 0xE9);
        let idle = Press { hover: 0.0, down: 0.0 };
        assert_eq!(idle.fill(rest, hover, pressed), rest);
        assert_eq!(idle.offset(), Vec2::ZERO);
        let held = Press { hover: 1.0, down: 1.0 };
        assert_eq!(held.fill(rest, hover, pressed), pressed);
        assert_eq!(held.offset(), Vec2::new(0.0, 1.0));
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
