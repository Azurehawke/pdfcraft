//! RevuCraft's symbol library: trade symbols stamped onto drawings, each with an optional label
//! ("PC", "VoIP", "D-101") underneath.
//!
//! A placed symbol is a custom stamp (§12.5.6.12) whose picture is a form XObject drawn here:
//! vector art in a 100-unit square, plus the label. Moving, resizing and deleting it work as for
//! any stamp, and other viewers show the same picture. It records what it is in
//! `/PCSymbol` (the symbol's id) and `/PCLabel`, which counts and the bill of materials read
//! back; `/Subj` is the symbol's name and `/Contents` the label, as other viewers show them.
//!
//! The artwork is original, drawn from the conventions of low-voltage plan symbols (outlets as
//! triangles, devices as circles and boxes with a short tag); it copies no product's symbol set.

use pdfcraft_cos::{Dict, Document, ObjRef, Object, PdfString, Stream};

use crate::{AnnotError, Meta, NewAnnotation, Rgb, Shape, Style, add_annotation, annot_ref, annots, page_refs};

/// The artwork's square, in its own units.
const BOX: f64 = 100.0;
/// Line weight in artwork units.
const WEIGHT: f64 = 6.0;
/// Label text size in artwork units, and the band it sits in under the symbol.
const LABEL_SIZE: f64 = 26.0;
const LABEL_BAND: f64 = 34.0;
/// Labels longer than this are cut (characters).
pub const MAX_LABEL: usize = 40;
/// Placed symbols counted at most.
const MAX_LISTED: usize = 200_000;

/// A symbol in the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Symbol {
    DataJack,
    DualJack,
    QuadJack,
    VoiceJack,
    WirelessAp,
    CameraDome,
    CameraBullet,
    PoeDevice,
    FloorBox,
    Rack,
    PatchPanel,
    Switch,
    FiberTermination,
    Demarc,
    Display,
    CardReader,
    Speaker,
    JHook,
    ConduitStub,
    DuplexOutlet,
    GfciOutlet,
    DedicatedOutlet,
    FloorOutlet,
    LightSwitch,
    ThreeWaySwitch,
    CeilingLight,
    WallLight,
    ExitSign,
    JunctionBox,
    ElectricalPanel,
    MotionSensor,
    DoorContact,
    GlassBreak,
    Keypad,
    Siren,
    SmokeDetector,
    HeatDetector,
    ElectricStrike,
    MagLock,
    RequestToExit,
}

/// A trade the library groups symbols under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trade {
    Networking,
    Electrical,
    Security,
}

impl Trade {
    pub const ALL: [Trade; 3] = [Trade::Networking, Trade::Electrical, Trade::Security];

    /// The English name, translated where it is shown.
    pub fn name(self) -> &'static str {
        match self {
            Self::Networking => "Networking",
            Self::Electrical => "Electrical",
            Self::Security => "Security",
        }
    }

    /// The stable id (settings, a symbol of your own's category).
    pub fn id(self) -> &'static str {
        match self {
            Self::Networking => "networking",
            Self::Electrical => "electrical",
            Self::Security => "security",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }
}

impl Symbol {
    pub const ALL: [Symbol; 40] = [
        Symbol::DataJack,
        Symbol::DualJack,
        Symbol::QuadJack,
        Symbol::VoiceJack,
        Symbol::WirelessAp,
        Symbol::CameraDome,
        Symbol::CameraBullet,
        Symbol::PoeDevice,
        Symbol::FloorBox,
        Symbol::Rack,
        Symbol::PatchPanel,
        Symbol::Switch,
        Symbol::FiberTermination,
        Symbol::Demarc,
        Symbol::Display,
        Symbol::CardReader,
        Symbol::Speaker,
        Symbol::JHook,
        Symbol::ConduitStub,
        Symbol::DuplexOutlet,
        Symbol::GfciOutlet,
        Symbol::DedicatedOutlet,
        Symbol::FloorOutlet,
        Symbol::LightSwitch,
        Symbol::ThreeWaySwitch,
        Symbol::CeilingLight,
        Symbol::WallLight,
        Symbol::ExitSign,
        Symbol::JunctionBox,
        Symbol::ElectricalPanel,
        Symbol::MotionSensor,
        Symbol::DoorContact,
        Symbol::GlassBreak,
        Symbol::Keypad,
        Symbol::Siren,
        Symbol::SmokeDetector,
        Symbol::HeatDetector,
        Symbol::ElectricStrike,
        Symbol::MagLock,
        Symbol::RequestToExit,
    ];

    /// The stable id written to `/PCSymbol`.
    pub fn id(self) -> &'static str {
        match self {
            Self::DataJack => "data-jack",
            Self::DualJack => "dual-jack",
            Self::QuadJack => "quad-jack",
            Self::VoiceJack => "voice-jack",
            Self::WirelessAp => "wireless-ap",
            Self::CameraDome => "camera-dome",
            Self::CameraBullet => "camera-bullet",
            Self::PoeDevice => "poe-device",
            Self::FloorBox => "floor-box",
            Self::Rack => "rack",
            Self::PatchPanel => "patch-panel",
            Self::Switch => "switch",
            Self::FiberTermination => "fiber",
            Self::Demarc => "demarc",
            Self::Display => "display",
            Self::CardReader => "card-reader",
            Self::Speaker => "speaker",
            Self::JHook => "j-hook",
            Self::ConduitStub => "conduit-stub",
            Self::DuplexOutlet => "duplex-outlet",
            Self::GfciOutlet => "gfci-outlet",
            Self::DedicatedOutlet => "dedicated-outlet",
            Self::FloorOutlet => "floor-outlet",
            Self::LightSwitch => "light-switch",
            Self::ThreeWaySwitch => "three-way-switch",
            Self::CeilingLight => "ceiling-light",
            Self::WallLight => "wall-light",
            Self::ExitSign => "exit-sign",
            Self::JunctionBox => "junction-box",
            Self::ElectricalPanel => "electrical-panel",
            Self::MotionSensor => "motion-sensor",
            Self::DoorContact => "door-contact",
            Self::GlassBreak => "glass-break",
            Self::Keypad => "keypad",
            Self::Siren => "siren",
            Self::SmokeDetector => "smoke-detector",
            Self::HeatDetector => "heat-detector",
            Self::ElectricStrike => "electric-strike",
            Self::MagLock => "mag-lock",
            Self::RequestToExit => "request-to-exit",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// The English name, translated where it is shown.
    pub fn name(self) -> &'static str {
        match self {
            Self::DataJack => "Data jack (RJ-45)",
            Self::DualJack => "Dual data jack",
            Self::QuadJack => "Quad data jack",
            Self::VoiceJack => "Voice jack",
            Self::WirelessAp => "Wireless access point",
            Self::CameraDome => "Camera (dome)",
            Self::CameraBullet => "Camera (bullet)",
            Self::PoeDevice => "PoE device",
            Self::FloorBox => "Floor box",
            Self::Rack => "Equipment rack",
            Self::PatchPanel => "Patch panel",
            Self::Switch => "Network switch",
            Self::FiberTermination => "Fiber termination",
            Self::Demarc => "Demarc",
            Self::Display => "Display / TV",
            Self::CardReader => "Card reader",
            Self::Speaker => "Speaker",
            Self::JHook => "J-hook",
            Self::ConduitStub => "Conduit stub-up",
            Self::DuplexOutlet => "Duplex outlet",
            Self::GfciOutlet => "GFCI outlet",
            Self::DedicatedOutlet => "Dedicated outlet",
            Self::FloorOutlet => "Floor outlet",
            Self::LightSwitch => "Light switch",
            Self::ThreeWaySwitch => "3-way switch",
            Self::CeilingLight => "Ceiling light",
            Self::WallLight => "Wall light",
            Self::ExitSign => "Exit sign",
            Self::JunctionBox => "Junction box",
            Self::ElectricalPanel => "Electrical panel",
            Self::MotionSensor => "Motion sensor",
            Self::DoorContact => "Door contact",
            Self::GlassBreak => "Glass-break detector",
            Self::Keypad => "Keypad",
            Self::Siren => "Siren",
            Self::SmokeDetector => "Smoke detector",
            Self::HeatDetector => "Heat detector",
            Self::ElectricStrike => "Electric strike",
            Self::MagLock => "Mag lock",
            Self::RequestToExit => "Request-to-exit button",
        }
    }

    /// The artwork's content operators in its 100-unit square, for previews.
    pub fn artwork(self) -> String {
        self.art()
    }

    pub fn trade(self) -> Trade {
        match self {
            Self::DuplexOutlet
            | Self::GfciOutlet
            | Self::DedicatedOutlet
            | Self::FloorOutlet
            | Self::LightSwitch
            | Self::ThreeWaySwitch
            | Self::CeilingLight
            | Self::WallLight
            | Self::ExitSign
            | Self::JunctionBox
            | Self::ElectricalPanel => Trade::Electrical,
            Self::MotionSensor
            | Self::DoorContact
            | Self::GlassBreak
            | Self::Keypad
            | Self::Siren
            | Self::SmokeDetector
            | Self::HeatDetector
            | Self::ElectricStrike
            | Self::MagLock
            | Self::RequestToExit => Trade::Security,
            _ => Trade::Networking,
        }
    }

    /// The artwork in the 100-unit square (y up), stroked and filled in the current colour:
    /// path operators and, for tags, text in `/F1` (Helvetica-Bold).
    fn art(self) -> String {
        let mut a = Art::default();
        match self {
            Self::DataJack => a.outlet("D"),
            Self::DualJack => a.outlet("2"),
            Self::QuadJack => a.outlet("4"),
            Self::VoiceJack => a.outlet("V"),
            Self::WirelessAp => {
                a.circle(50.0, 42.0, 24.0);
                a.tag(50.0, 42.0, 18.0, "AP");
                a.arc_up(50.0, 66.0, 14.0);
                a.arc_up(50.0, 66.0, 26.0);
            }
            Self::CameraDome => {
                a.circle(50.0, 50.0, 40.0);
                a.dome(50.0, 38.0, 22.0);
                a.tag(50.0, 66.0, 16.0, "CAM");
            }
            Self::CameraBullet => {
                a.rect(14.0, 38.0, 56.0, 30.0);
                a.poly(&[(70.0, 44.0), (88.0, 34.0), (88.0, 72.0), (70.0, 62.0)]);
                a.line(30.0, 38.0, 30.0, 20.0);
                a.line(18.0, 20.0, 46.0, 20.0);
            }
            Self::PoeDevice => {
                a.rect(10.0, 26.0, 80.0, 48.0);
                a.tag(50.0, 50.0, 26.0, "PoE");
            }
            Self::FloorBox => {
                a.rect(16.0, 16.0, 68.0, 68.0);
                a.line(16.0, 16.0, 84.0, 84.0);
                a.line(16.0, 84.0, 84.0, 16.0);
            }
            Self::Rack => {
                a.rect(26.0, 6.0, 48.0, 88.0);
                for y in [24.0, 40.0, 56.0, 72.0] {
                    a.line(26.0, y, 74.0, y);
                }
            }
            Self::PatchPanel => {
                a.rect(6.0, 30.0, 88.0, 40.0);
                for x in [16.0, 30.0, 44.0, 58.0, 72.0] {
                    a.fill_rect(x, 44.0, 10.0, 12.0);
                }
            }
            Self::Switch => {
                a.rect(6.0, 30.0, 88.0, 40.0);
                a.tag(50.0, 50.0, 26.0, "SW");
            }
            Self::FiberTermination => {
                a.circle(50.0, 50.0, 38.0);
                a.tag(50.0, 50.0, 28.0, "FO");
            }
            Self::Demarc => {
                a.poly(&[(30.0, 14.0), (70.0, 14.0), (90.0, 50.0), (70.0, 86.0), (30.0, 86.0), (10.0, 50.0)]);
                a.tag(50.0, 50.0, 26.0, "DM");
            }
            Self::Display => {
                a.rect(8.0, 30.0, 84.0, 52.0);
                a.line(50.0, 30.0, 50.0, 16.0);
                a.line(30.0, 14.0, 70.0, 14.0);
                a.tag(50.0, 56.0, 24.0, "TV");
            }
            Self::CardReader => {
                a.rect(28.0, 10.0, 44.0, 80.0);
                a.tag(50.0, 60.0, 22.0, "AC");
                a.circle(50.0, 28.0, 7.0);
            }
            Self::Speaker => {
                a.circle(50.0, 50.0, 40.0);
                a.poly(&[(32.0, 40.0), (44.0, 40.0), (60.0, 26.0), (60.0, 74.0), (44.0, 60.0), (32.0, 60.0)]);
            }
            Self::JHook => {
                a.raw("50 88 m 50 34 l 50 14 70 14 70 34 c S\n");
                a.line(36.0, 88.0, 64.0, 88.0);
            }
            Self::ConduitStub => {
                a.circle(50.0, 40.0, 26.0);
                a.line(50.0, 66.0, 50.0, 94.0);
                a.poly(&[(38.0, 82.0), (50.0, 96.0), (62.0, 82.0)]);
                a.tag(50.0, 40.0, 22.0, "C");
            }
            Self::DuplexOutlet => a.receptacle(),
            Self::GfciOutlet => {
                a.receptacle();
                a.tag(50.0, 12.0, 15.0, "GFCI");
            }
            Self::DedicatedOutlet => {
                a.receptacle();
                a.fill_poly(&[(38.0, 86.0), (62.0, 86.0), (50.0, 74.0)]);
            }
            Self::FloorOutlet => {
                a.rect(14.0, 14.0, 72.0, 72.0);
                a.circle(50.0, 50.0, 22.0);
                a.line(30.0, 44.0, 70.0, 44.0);
                a.line(30.0, 56.0, 70.0, 56.0);
            }
            Self::LightSwitch => a.tag(50.0, 50.0, 56.0, "S"),
            Self::ThreeWaySwitch => {
                a.tag(42.0, 54.0, 52.0, "S");
                a.tag(72.0, 30.0, 26.0, "3");
            }
            Self::CeilingLight => {
                a.circle(50.0, 50.0, 30.0);
                a.line(29.0, 29.0, 71.0, 71.0);
                a.line(29.0, 71.0, 71.0, 29.0);
            }
            Self::WallLight => {
                a.circle(50.0, 44.0, 24.0);
                a.line(33.0, 27.0, 67.0, 61.0);
                a.line(33.0, 61.0, 67.0, 27.0);
                a.line(50.0, 68.0, 50.0, 90.0);
                a.line(28.0, 90.0, 72.0, 90.0);
            }
            Self::ExitSign => {
                a.rect(8.0, 34.0, 84.0, 36.0);
                a.tag(50.0, 52.0, 22.0, "EXIT");
                a.poly(&[(40.0, 22.0), (60.0, 22.0), (60.0, 16.0), (72.0, 26.0), (60.0, 36.0), (60.0, 30.0), (40.0, 30.0)]);
            }
            Self::JunctionBox => {
                a.circle(50.0, 50.0, 34.0);
                a.tag(50.0, 50.0, 34.0, "J");
            }
            Self::ElectricalPanel => {
                a.rect(8.0, 34.0, 84.0, 32.0);
                a.fill_poly(&[(8.0, 34.0), (92.0, 34.0), (8.0, 66.0)]);
            }
            Self::MotionSensor => {
                a.circle(50.0, 40.0, 26.0);
                a.tag(50.0, 40.0, 22.0, "MD");
                a.arc_up(50.0, 66.0, 12.0);
                a.arc_up(50.0, 66.0, 22.0);
            }
            Self::DoorContact => {
                a.rect(16.0, 36.0, 30.0, 28.0);
                a.rect(54.0, 36.0, 30.0, 28.0);
                a.tag(50.0, 82.0, 20.0, "DC");
            }
            Self::GlassBreak => {
                a.circle(50.0, 50.0, 36.0);
                a.tag(50.0, 50.0, 28.0, "GB");
            }
            Self::Keypad => {
                a.rect(22.0, 8.0, 56.0, 84.0);
                for y in [16.0, 34.0, 52.0] {
                    for x in [30.0, 45.0, 60.0] {
                        a.fill_rect(x, y, 10.0, 10.0);
                    }
                }
                a.rect(30.0, 70.0, 40.0, 14.0);
            }
            Self::Siren => {
                a.poly(&[(14.0, 38.0), (34.0, 38.0), (70.0, 14.0), (70.0, 86.0), (34.0, 62.0), (14.0, 62.0)]);
                a.line(78.0, 50.0, 94.0, 50.0);
                a.line(78.0, 66.0, 92.0, 76.0);
                a.line(78.0, 34.0, 92.0, 24.0);
            }
            Self::SmokeDetector => {
                a.circle(50.0, 50.0, 38.0);
                a.tag(50.0, 50.0, 28.0, "SD");
            }
            Self::HeatDetector => {
                a.circle(50.0, 50.0, 38.0);
                a.tag(50.0, 50.0, 28.0, "HD");
            }
            Self::ElectricStrike => {
                a.rect(30.0, 10.0, 40.0, 80.0);
                a.tag(50.0, 50.0, 24.0, "ES");
            }
            Self::MagLock => {
                a.rect(8.0, 34.0, 84.0, 32.0);
                a.tag(50.0, 50.0, 24.0, "ML");
            }
            Self::RequestToExit => {
                a.circle(50.0, 50.0, 40.0);
                a.tag(50.0, 50.0, 24.0, "REX");
            }
        }
        a.0
    }
}

/// Artwork content, built up from simple shapes.
#[derive(Default)]
struct Art(String);

fn n(v: f64) -> String {
    crate::n(v)
}

impl Art {
    fn raw(&mut self, s: &str) {
        self.0.push_str(s);
    }
    fn line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.0.push_str(&format!("{} {} m {} {} l S\n", n(x0), n(y0), n(x1), n(y1)));
    }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.0.push_str(&format!("{} {} {} {} re S\n", n(x), n(y), n(w), n(h)));
    }
    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.0.push_str(&format!("{} {} {} {} re f\n", n(x), n(y), n(w), n(h)));
    }
    fn poly(&mut self, pts: &[(f64, f64)]) {
        for (i, (x, y)) in pts.iter().enumerate() {
            self.0.push_str(&format!("{} {} {}\n", n(*x), n(*y), if i == 0 { "m" } else { "l" }));
        }
        self.0.push_str("h S\n");
    }
    fn fill_poly(&mut self, pts: &[(f64, f64)]) {
        for (i, (x, y)) in pts.iter().enumerate() {
            self.0.push_str(&format!("{} {} {}\n", n(*x), n(*y), if i == 0 { "m" } else { "l" }));
        }
        self.0.push_str("h f\n");
    }
    /// An ellipse centred on (cx, cy) with radii rx, ry.
    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64) {
        let (kx, ky) = (rx * 0.552_284_75, ry * 0.552_284_75);
        let c = |v: [f64; 6]| format!("{} {} {} {} {} {} c ", n(v[0]), n(v[1]), n(v[2]), n(v[3]), n(v[4]), n(v[5]));
        self.0.push_str(&format!("{} {} m ", n(cx + rx), n(cy)));
        self.0.push_str(&c([cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry]));
        self.0.push_str(&c([cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy]));
        self.0.push_str(&c([cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry]));
        self.0.push_str(&c([cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy]));
        self.0.push_str("h S\n");
    }
    /// A receptacle: a circle with two lines through it.
    fn receptacle(&mut self) {
        self.circle(50.0, 50.0, 24.0);
        self.line(12.0, 44.0, 88.0, 44.0);
        self.line(12.0, 56.0, 88.0, 56.0);
    }
    fn circle(&mut self, cx: f64, cy: f64, r: f64) {
        let k = r * 0.552_284_75;
        self.0.push_str(&format!(
            "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c h S\n",
            n(cx + r),
            n(cy),
            n(cx + r),
            n(cy + k),
            n(cx + k),
            n(cy + r),
            n(cx),
            n(cy + r),
            n(cx - k),
            n(cy + r),
            n(cx - r),
            n(cy + k),
            n(cx - r),
            n(cy),
            n(cx - r),
            n(cy - k),
            n(cx - k),
            n(cy - r),
            n(cx),
            n(cy - r),
            n(cx + k),
            n(cy - r),
            n(cx + r),
            n(cy - k),
            n(cx + r),
            n(cy)
        ));
    }
    /// The upper half of a circle, as a signal arc.
    fn arc_up(&mut self, cx: f64, cy: f64, r: f64) {
        let k = r * 0.552_284_75;
        self.0.push_str(&format!(
            "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c S\n",
            n(cx - r),
            n(cy),
            n(cx - r),
            n(cy + k),
            n(cx - k),
            n(cy + r),
            n(cx),
            n(cy + r),
            n(cx + k),
            n(cy + r),
            n(cx + r),
            n(cy + k),
            n(cx + r),
            n(cy)
        ));
    }
    /// A filled dome: the lower half of a circle, flat side up.
    fn dome(&mut self, cx: f64, cy: f64, r: f64) {
        let k = r * 0.552_284_75;
        self.0.push_str(&format!(
            "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c h f\n",
            n(cx - r),
            n(cy),
            n(cx - r),
            n(cy - k),
            n(cx - k),
            n(cy - r),
            n(cx),
            n(cy - r),
            n(cx + k),
            n(cy - r),
            n(cx + r),
            n(cy - k),
            n(cx + r),
            n(cy)
        ));
    }
    /// An outlet: a triangle pointing down with its tag inside.
    fn outlet(&mut self, tag: &str) {
        self.poly(&[(10.0, 88.0), (90.0, 88.0), (50.0, 14.0)]);
        self.tag(50.0, 62.0, 30.0, tag);
    }
    /// A short tag centred on (x, y).
    fn tag(&mut self, x: f64, y: f64, size: f64, text: &str) {
        let w = pdfcraft_fonts::helvetica_width(text, size) * 1.06;
        self.0.push_str(&format!(
            "BT /F1 {} Tf {} {} Td ({}) Tj ET\n",
            n(size),
            n(x - w / 2.0),
            n(y - size * 0.35),
            text.replace('\\', "").replace(['(', ')'], "")
        ));
    }
}

/// One of your own symbols: its name and its artwork (from an SVG, [`crate::svg_art::svg_art`],
/// or from drafted lines, [`art_from_drawing`]), and the trade it is listed under (`None`: My
/// symbols).
#[derive(Clone, Debug, PartialEq)]
pub struct CustomArt {
    pub name: String,
    pub art: String,
    pub trade: Option<Trade>,
}

/// Points read from the drawing for one symbol, at most.
const MAX_DRAWING_POINTS: usize = 20_000;

/// One drawn part of a selection, in user space.
enum Part {
    /// An open or closed run of points.
    Path(Vec<[f64; 2]>, bool),
    /// A rectangle or an ellipse in it.
    Rect([f64; 4], bool),
}

/// The artwork for a symbol of your own made from drawn lines and shapes on `page` (lines,
/// polylines, polygons, rectangles, ellipses, ink): fitted into the symbol's square, centred,
/// proportions kept. Errors when none of them is a line or shape.
pub fn art_from_drawing(doc: &Document, page: usize, indices: &[usize]) -> Result<String, AnnotError> {
    let p = crate::page_ref(doc, page)?;
    let list = annots(doc, p);
    let mut parts: Vec<Part> = Vec::new();
    let mut points = 0usize;
    let nums = |d: &Dict, key: &[u8]| -> Vec<f64> {
        d.get(key)
            .map(|o| doc.resolve(o))
            .and_then(|o| o.as_array().map(|a| a.iter().filter_map(Object::as_f64).filter(|v| v.is_finite()).collect()))
            .unwrap_or_default()
    };
    let pairs = |v: &[f64]| -> Vec<[f64; 2]> { v.as_chunks::<2>().0.iter().map(|c| [c[0], c[1]]).collect() };
    for &i in indices {
        let Some(entry) = list.get(i) else { continue };
        let obj = doc.resolve(entry);
        let Some(d) = obj.as_dict() else { continue };
        let part = match d.name(b"Subtype") {
            Some(b"Line") => Some(Part::Path(pairs(&nums(d, b"L")), false)),
            Some(b"PolyLine") => Some(Part::Path(pairs(&nums(d, b"Vertices")), false)),
            Some(b"Polygon") => Some(Part::Path(pairs(&nums(d, b"Vertices")), true)),
            Some(kind @ (b"Square" | b"Circle")) => {
                let r = nums(d, b"Rect");
                let rd = nums(d, b"RD");
                match (r.as_slice(), rd.as_slice()) {
                    ([x0, y0, x1, y1], rd) => {
                        let (l, b, rt, t) = match rd {
                            [a, b, c, e] => (*a, *b, *c, *e),
                            _ => (0.0, 0.0, 0.0, 0.0),
                        };
                        let rect = [x0.min(*x1) + l, y0.min(*y1) + b, x0.max(*x1) - rt, y0.max(*y1) - t];
                        (rect[2] > rect[0] && rect[3] > rect[1]).then_some(Part::Rect(rect, kind == b"Circle"))
                    }
                    _ => None,
                }
            }
            Some(b"Ink") => {
                let strokes = d.get(b"InkList").map(|o| doc.resolve(o)).and_then(|o| o.as_array().cloned()).unwrap_or_default();
                for stroke in strokes {
                    let v: Vec<f64> = doc
                        .resolve(&stroke)
                        .as_array()
                        .map(|a| a.iter().filter_map(Object::as_f64).filter(|v| v.is_finite()).collect())
                        .unwrap_or_default();
                    let path = pairs(&v);
                    points = points.saturating_add(path.len());
                    if path.len() >= 2 && points <= MAX_DRAWING_POINTS {
                        parts.push(Part::Path(path, false));
                    }
                }
                None
            }
            _ => None,
        };
        match part {
            Some(Part::Path(path, closed)) if path.len() >= 2 => {
                points = points.saturating_add(path.len());
                if points <= MAX_DRAWING_POINTS {
                    parts.push(Part::Path(path, closed));
                }
            }
            Some(r @ Part::Rect(..)) => parts.push(r),
            _ => {}
        }
    }
    // The bounds of it all.
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut grow = |x: f64, y: f64| {
        b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
    };
    for part in &parts {
        match part {
            Part::Path(path, _) => path.iter().for_each(|q| grow(q[0], q[1])),
            Part::Rect(r, _) => {
                grow(r[0], r[1]);
                grow(r[2], r[3]);
            }
        }
    }
    let span = (b[2] - b[0]).max(b[3] - b[1]);
    if parts.is_empty() || !span.is_finite() || span <= 1e-6 {
        return Err(AnnotError::Invalid("choose drawn lines or shapes to make a symbol from".into()));
    }
    // Fitted into the square with a margin for the line weight.
    let scale = (BOX - 2.0 * WEIGHT) / span;
    let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    let at = |x: f64, y: f64| ((x - cx) * scale + BOX / 2.0, (y - cy) * scale + BOX / 2.0);
    let mut a = Art::default();
    for part in &parts {
        match part {
            Part::Path(path, closed) => {
                let pts: Vec<(f64, f64)> = path.iter().map(|q| at(q[0], q[1])).collect();
                for (i, (x, y)) in pts.iter().enumerate() {
                    a.raw(&format!("{} {} {}\n", n(*x), n(*y), if i == 0 { "m" } else { "l" }));
                }
                a.raw(if *closed { "h S\n" } else { "S\n" });
            }
            Part::Rect(r, ellipse) => {
                let (x0, y0) = at(r[0], r[1]);
                let (x1, y1) = at(r[2], r[3]);
                if *ellipse {
                    a.ellipse((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0, (y1 - y0) / 2.0);
                } else {
                    a.rect(x0, y0, x1 - x0, y1 - y0);
                }
            }
        }
    }
    if a.0.len() > crate::svg_art::MAX_ART {
        return Err(AnnotError::Invalid("the drawing is too detailed for a symbol".into()));
    }
    Ok(a.0)
}

/// The id a symbol of your own is recorded under (`/PCSymbol`).
pub fn custom_id(name: &str) -> String {
    format!("custom:{}", clean_label(name))
}

/// A symbol to place.
#[derive(Clone, Debug, PartialEq)]
pub struct NewSymbol {
    pub page: usize,
    pub symbol: Symbol,
    /// Where the symbol's centre goes, in user space.
    pub center: [f64; 2],
    /// The symbol's width in user units.
    pub size: f64,
    /// The text under it; empty for none.
    pub label: String,
    pub color: Rgb,
    pub author: String,
    /// Its turn in degrees anticlockwise (0: level).
    pub rotation: f64,
    /// One of your own symbols, drawn instead of `symbol`.
    pub custom: Option<CustomArt>,
}

fn rg(c: Rgb) -> String {
    format!("{} {} {} RG {} {} {} rg\n", n(c[0]), n(c[1]), n(c[2]), n(c[0]), n(c[1]), n(c[2]))
}

fn font(base: &str) -> Object {
    let mut f = Dict::new();
    f.set(b"Type".to_vec(), Object::name("Font"));
    f.set(b"Subtype".to_vec(), Object::name("Type1"));
    f.set(b"BaseFont".to_vec(), Object::name(base));
    f.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
    Object::Dict(f)
}

/// The side of the square the artwork is drawn in: the box itself, or room for any turn of it.
fn art_side(degrees: f64) -> f64 {
    if degrees.rem_euclid(360.0).abs() < 1e-9 { BOX } else { BOX * std::f64::consts::SQRT_2 }
}

/// The picture of `symbol` with `label`, its artwork turned `degrees` anticlockwise (the label
/// stays level): a form XObject, and its width and height in its units.
fn picture(art: &str, label: &str, color: Rgb, degrees: f64) -> (Stream, f64, f64) {
    let label_width = if label.is_empty() { 0.0 } else { pdfcraft_fonts::helvetica_width(label, LABEL_SIZE) * 1.04 + 8.0 };
    let side = art_side(degrees);
    let width = side.max(label_width);
    let band = if label.is_empty() { 0.0 } else { LABEL_BAND };
    let height = side + band;
    let mut content: Vec<u8> = Vec::new();
    content.extend_from_slice(rg(color).as_bytes());
    let (sin, cos) = degrees.to_radians().sin_cos();
    content.extend_from_slice(
        format!(
            "{} w 1 J 1 j\nq 1 0 0 1 {} {} cm {} {} {} {} 0 0 cm 1 0 0 1 {} {} cm\n",
            n(WEIGHT),
            n(width / 2.0),
            n(band + side / 2.0),
            n(cos),
            n(sin),
            n(-sin),
            n(cos),
            n(-BOX / 2.0),
            n(-BOX / 2.0)
        )
        .as_bytes(),
    );
    content.extend_from_slice(art.as_bytes());
    content.extend_from_slice(b"Q\n");
    if !label.is_empty() {
        let text = pdfcraft_fonts::win_ansi(label);
        content.extend_from_slice(
            format!("BT /F2 {} Tf {} {} Td ", n(LABEL_SIZE), n((width - label_width + 8.0) / 2.0), n(LABEL_SIZE * 0.3)).as_bytes(),
        );
        content.extend_from_slice(&pdfcraft_fonts::literal(&text));
        content.extend_from_slice(b" Tj ET\n");
    }
    let mut fonts = Dict::new();
    fonts.set(b"F1".to_vec(), font("Helvetica-Bold"));
    fonts.set(b"F2".to_vec(), font("Helvetica"));
    let mut res = Dict::new();
    res.set(b"Font".to_vec(), Object::Dict(fonts));
    let mut d = Dict::new();
    d.set(b"Type".to_vec(), Object::name("XObject"));
    d.set(b"Subtype".to_vec(), Object::name("Form"));
    d.set(b"FormType".to_vec(), Object::Int(1));
    d.set(b"BBox".to_vec(), Object::Array([0.0, 0.0, width, height].iter().map(|v| Object::Real(*v)).collect()));
    d.set(b"Resources".to_vec(), Object::Dict(res));
    (Stream::flate(d, &content), width, height)
}

/// Clean a label: trimmed, printable, at most [`MAX_LABEL`] characters.
pub fn clean_label(label: &str) -> String {
    let printable: String = label.chars().filter(|c| !c.is_control()).collect();
    printable.trim().chars().take(MAX_LABEL).collect()
}

/// Place a symbol. Returns its index in the page's annotations.
pub fn add_symbol(doc: &mut Document, new: &NewSymbol, meta: &Meta) -> Result<usize, AnnotError> {
    if !new.center.iter().all(|v| v.is_finite() && v.abs() < 1e7) {
        return Err(AnnotError::Invalid("invalid symbol position".into()));
    }
    if !new.size.is_finite() || !(0.5..=20_000.0).contains(&new.size) {
        return Err(AnnotError::Invalid("a symbol's size must be between 0.5 and 20000 points".into()));
    }
    if !new.color.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)) {
        return Err(AnnotError::Invalid("invalid symbol colour".into()));
    }
    let label = clean_label(&new.label);
    if !new.rotation.is_finite() || new.rotation.abs() > 36_000.0 {
        return Err(AnnotError::Invalid("invalid symbol turn".into()));
    }
    let turn = new.rotation.rem_euclid(360.0);
    let (id, name, art) = match &new.custom {
        Some(c) => {
            let name = clean_label(&c.name);
            if name.is_empty() || c.art.is_empty() || c.art.len() > crate::svg_art::MAX_ART {
                return Err(AnnotError::Invalid("that symbol of your own can't be drawn".into()));
            }
            (custom_id(&name), name, c.art.clone())
        }
        None => (new.symbol.id().to_string(), new.symbol.name().to_string(), new.symbol.art()),
    };
    let (stream, width, height) = picture(&art, &label, new.color, turn);
    let picture = doc.add(Object::Stream(stream));
    // Your own symbol keeps its artwork, so it can be turned and relabelled later.
    let art_ref = new.custom.is_some().then(|| doc.add(Object::Stream(Stream::flate(Dict::new(), art.as_bytes()))));
    let k = new.size / BOX;
    let band = if label.is_empty() { 0.0 } else { LABEL_BAND };
    // The symbol's centre (not the label's) lands on the point.
    let x0 = new.center[0] - width / 2.0 * k;
    let y0 = new.center[1] - (band + art_side(turn) / 2.0) * k;
    let rect = [x0, y0, x0 + width * k, y0 + height * k];
    let stamp_name = if new.custom.is_some() { "SymbolCustom".to_string() } else { format!("Symbol{}", new.symbol.id()) };
    let shape = Shape::CustomStamp { rect, name: stamp_name, picture, image: false, size: (width, height) };
    let annotation = NewAnnotation { page: new.page, style: Style::default_for(&shape), shape, contents: label.clone(), author: new.author.clone() };
    let index = add_annotation(doc, &annotation, meta)?;
    let (_, r) = annot_ref(doc, new.page, index)?;
    doc.update_dict(r, |d| {
        d.set(b"Subj".to_vec(), PdfString::text(&name));
        d.set(b"PCSymbol".to_vec(), PdfString::text(&id));
        if let Some(r) = art_ref {
            d.set(b"PCArt".to_vec(), Object::Ref(r));
        }
        d.set(b"PCLabel".to_vec(), PdfString::text(&label));
        d.set(b"PCSize".to_vec(), Object::Real(new.size));
        if turn != 0.0 {
            d.set(b"PCRotation".to_vec(), Object::Real(turn));
        }
        d.set(b"C".to_vec(), Object::Array(new.color.iter().map(|v| Object::Real(*v)).collect()));
    })?;
    Ok(index)
}

/// Change a placed symbol's label and/or turn (degrees anticlockwise, absolute), keeping its size
/// and the point its centre sits on. The label stays level as the artwork turns.
pub fn edit_symbol(doc: &mut Document, page: usize, index: usize, label: Option<&str>, degrees: Option<f64>, meta: &Meta) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    crate::unlocked(doc, r)?;
    let d = crate::annot_dict(doc, r);
    let text = |k: &[u8]| d.get(k).map(|o| doc.resolve(o)).and_then(|o| o.as_string().map(PdfString::to_text));
    let id = text(b"PCSymbol").unwrap_or_default();
    let art = match Symbol::from_id(&id) {
        Some(symbol) => symbol.art(),
        // Your own symbol: the artwork kept with it.
        None if id.starts_with("custom:") => {
            let stored = d.get(b"PCArt").map(|o| doc.resolve(o));
            match stored.as_deref() {
                Some(Object::Stream(s)) => s
                    .decoded_within(crate::svg_art::MAX_ART)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                    .ok_or_else(|| AnnotError::Invalid("the symbol's artwork can't be read".into()))?,
                _ => return Err(AnnotError::Invalid("the symbol's artwork is missing".into())),
            }
        }
        None => return Err(AnnotError::Invalid("not a symbol from the library".into())),
    };
    let nums = |k: &[u8]| -> Vec<f64> {
        d.get(k).map(|o| doc.resolve(o)).and_then(|o| o.as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).collect())).unwrap_or_default()
    };
    let color: Rgb = match nums(b"C")[..] {
        [r, g, b] => [r, g, b].map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }),
        _ => [0.0; 3],
    };
    let old_label = text(b"PCLabel").unwrap_or_default();
    let old_turn = d.get(b"PCRotation").and_then(|o| doc.resolve(o).as_f64()).filter(|v| v.is_finite()).unwrap_or(0.0);
    let [x0, y0, x1, y1] = nums(b"Rect")[..] else { return Err(AnnotError::Invalid("the symbol has no rectangle".into())) };
    let (x0, y0, x1, y1) = (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1));
    let old_band = if old_label.is_empty() { 0.0 } else { LABEL_BAND };
    let old_height = art_side(old_turn) + old_band;
    // Its scale (user units per picture unit), from its size, else from its rectangle.
    let k = match d.get(b"PCSize").and_then(|o| doc.resolve(o).as_f64()).filter(|v| v.is_finite() && *v > 0.0) {
        Some(size) => size / BOX,
        None => (y1 - y0) / old_height,
    };
    if !k.is_finite() || k <= 0.0 {
        return Err(AnnotError::Invalid("the symbol's size can't be read".into()));
    }
    let center = [(x0 + x1) / 2.0, y0 + (old_band + art_side(old_turn) / 2.0) * k];
    let label = label.map_or(old_label, clean_label);
    let turn = match degrees {
        Some(t) if t.is_finite() && t.abs() <= 36_000.0 => t.rem_euclid(360.0),
        Some(_) => return Err(AnnotError::Invalid("invalid turn".into())),
        None => old_turn,
    };
    let (stream, width, height) = picture(&art, &label, color, turn);
    let pic = doc.add(Object::Stream(stream));
    let band = if label.is_empty() { 0.0 } else { LABEL_BAND };
    let nx0 = center[0] - width / 2.0 * k;
    let ny0 = center[1] - (band + art_side(turn) / 2.0) * k;
    let rect = [nx0, ny0, nx0 + width * k, ny0 + height * k];
    if !rect.iter().all(|v| v.is_finite() && v.abs() < 1e7) {
        return Err(AnnotError::Invalid("the symbol would leave the page".into()));
    }
    doc.update_dict(r, |d| {
        d.set(b"Rect".to_vec(), Object::Array(rect.iter().map(|v| Object::Real(*v)).collect()));
        d.set(b"PCPicture".to_vec(), Object::Ref(pic));
        d.set(b"PCPictureSize".to_vec(), Object::Array(vec![Object::Real(width), Object::Real(height)]));
        d.set(b"PCLabel".to_vec(), PdfString::text(&label));
        d.set(b"Contents".to_vec(), PdfString::text(&label));
        d.set(b"PCSize".to_vec(), Object::Real(k * BOX));
        if turn == 0.0 {
            d.remove(b"PCRotation");
        } else {
            d.set(b"PCRotation".to_vec(), Object::Real(turn));
        }
        crate::touch(d, meta);
    })?;
    crate::set_appearance(doc, r)
}

/// The turn of the symbol at `(page, index)` in degrees (0 when level or not a symbol).
pub fn rotation_at(doc: &Document, page: usize, index: usize) -> f64 {
    let Some(p) = page_refs(doc).ok().and_then(|r| r.get(page).copied()) else { return 0.0 };
    annots(doc, p).get(index).map(|e| doc.resolve(e)).and_then(|o| o.as_dict().map(rotation_of)).unwrap_or(0.0)
}

/// A placed symbol's turn in degrees (0 when level).
pub fn rotation_of(d: &Dict) -> f64 {
    d.get(b"PCRotation").and_then(Object::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0)
}

/// A placed symbol, for counts and the bill of materials.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub page: usize,
    pub index: usize,
    /// The symbol's id (`/PCSymbol`), kept even when this build doesn't know it.
    pub symbol: String,
    pub label: String,
    /// The layer it is on, if any.
    pub layer: Option<ObjRef>,
}

/// Every placed symbol, page by page.
pub fn placed(doc: &Document) -> Vec<Placed> {
    let mut out = Vec::new();
    for (page, p) in page_refs(doc).unwrap_or_default().into_iter().enumerate() {
        for (index, entry) in annots(doc, p).iter().enumerate() {
            if out.len() >= MAX_LISTED {
                return out;
            }
            let obj = doc.resolve(entry);
            let Some(d) = obj.as_dict() else { continue };
            let text = |k: &[u8]| d.get(k).map(|o| doc.resolve(o)).and_then(|o| o.as_string().map(PdfString::to_text));
            let Some(symbol) = text(b"PCSymbol") else { continue };
            out.push(Placed { page, index, symbol, label: text(b"PCLabel").unwrap_or_default(), layer: crate::layers::layer_of(d) });
        }
    }
    out
}

/// The layer the annotation at (page, index) is on.
pub fn layer_at(doc: &Document, page: usize, index: usize) -> Option<ObjRef> {
    let p = page_refs(doc).ok()?.get(page).copied()?;
    let entry = annots(doc, p).get(index).cloned()?;
    let obj = doc.resolve(&entry);
    crate::layers::layer_of(obj.as_dict()?)
}

/// Points worth snapping to in the markup on `page`: line ends, polyline and polygon vertices,
/// rectangle corners and the centres of stamps and symbols (RevuCraft's drafting snaps).
pub fn markup_vertices(doc: &Document, page: usize) -> Vec<[f64; 2]> {
    const MAX: usize = 50_000;
    let mut out = Vec::new();
    let Some(p) = page_refs(doc).ok().and_then(|r| r.get(page).copied()) else { return out };
    for entry in annots(doc, p) {
        let obj = doc.resolve(&entry);
        let Some(d) = obj.as_dict() else { continue };
        let nums = |k: &[u8]| -> Vec<f64> {
            d.get(k)
                .map(|o| doc.resolve(o))
                .and_then(|o| o.as_array().map(|a| a.iter().filter_map(|x| doc.resolve(x).as_f64()).filter(|v| v.is_finite()).collect()))
                .unwrap_or_default()
        };
        match d.name(b"Subtype") {
            Some(b"Line") => out.extend(nums(b"L").as_chunks::<2>().0.iter().copied()),
            Some(b"PolyLine" | b"Polygon") => out.extend(nums(b"Vertices").as_chunks::<2>().0.iter().copied()),
            Some(b"Square") => {
                if let [x0, y0, x1, y1] = nums(b"Rect")[..] {
                    out.extend([[x0, y0], [x1, y0], [x1, y1], [x0, y1]]);
                }
            }
            Some(b"Stamp") => {
                if let [x0, y0, x1, y1] = nums(b"Rect")[..] {
                    out.push([(x0 + x1) / 2.0, (y0 + y1) / 2.0]);
                }
            }
            _ => {}
        }
        if out.len() > MAX {
            out.truncate(MAX);
            break;
        }
    }
    out
}

/// A spreadsheet cell: quoted when it has to be, and never read as a formula.
fn cell(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{text}") } else { text.to_string() };
    if text.contains([',', '"', '\n', '\r']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text }
}

/// The name a symbol id is shown by: the library's, or the id when this build doesn't know it.
pub fn display_name(id: &str) -> String {
    if let Some(name) = id.strip_prefix("custom:") {
        return name.to_string();
    }
    Symbol::from_id(id).map_or_else(|| id.to_string(), |s| s.name().to_string())
}

/// What counts are grouped by: symbol id, label and layer.
type GroupKey = (String, String, Option<ObjRef>);

/// Symbol counts as CSV: a summary (symbol, label, layer, count, pages), then every placement.
pub fn counts_csv(placed: &[Placed], layers: &[crate::layers::LayerInfo]) -> String {
    let layer_name = |l: Option<ObjRef>| l.and_then(|id| layers.iter().find(|x| x.id == id)).map(|x| x.name.clone()).unwrap_or_default();
    // Groups in the order they first appear.
    // (symbol, label, layer), how many, and the pages they are on.
    let mut groups: Vec<(GroupKey, usize, Vec<usize>)> = Vec::new();
    for p in placed {
        let key = (p.symbol.clone(), p.label.clone(), p.layer);
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => {
                g.1 += 1;
                if !g.2.contains(&p.page) {
                    g.2.push(p.page);
                }
            }
            None => groups.push((key, 1, vec![p.page])),
        }
    }
    let mut out = String::from("Symbol,Label,Layer,Count,Pages\r\n");
    for ((symbol, label, layer), count, pages) in &groups {
        let pages: Vec<String> = pages.iter().map(|p| (p + 1).to_string()).collect();
        out.push_str(&format!(
            "{},{},{},{},{}\r\n",
            cell(&display_name(symbol)),
            cell(label),
            cell(&layer_name(*layer)),
            count,
            cell(&pages.join(" "))
        ));
    }
    out.push_str(&format!("Total,,,{},\r\n\r\nPage,Symbol,Label,Layer\r\n", placed.len()));
    for p in placed {
        out.push_str(&format!("{},{},{},{}\r\n", p.page + 1, cell(&display_name(&p.symbol)), cell(&p.label), cell(&layer_name(p.layer))));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_cos::{SaveOptions, write_full};
    use std::sync::Arc;

    fn blank() -> Document {
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 600 400] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";
        Document::open(Arc::new(pdf.to_vec())).unwrap()
    }

    #[test]
    fn every_symbol_has_an_id_name_and_artwork() {
        let mut ids = std::collections::HashSet::new();
        for s in Symbol::ALL {
            assert!(ids.insert(s.id()), "{s:?} id repeats");
            assert_eq!(Symbol::from_id(s.id()), Some(s));
            assert!(!s.name().is_empty());
            let art = s.art();
            assert!(art.contains(" S\n") || art.contains(" f\n"), "{s:?} draws something");
            assert_eq!(art.matches("BT").count(), art.matches("ET").count());
        }
        // Every trade has symbols; the ids of trades read back.
        for t in Trade::ALL {
            assert!(Symbol::ALL.iter().any(|s| s.trade() == t), "{t:?}");
            assert_eq!(Trade::from_id(t.id()), Some(t));
        }
    }

    #[test]
    fn drawn_lines_make_a_symbol_fitted_to_its_square() {
        let mut doc = blank();
        let style = Style { color: [0.0, 0.0, 0.0], opacity: 1.0, width: 1.0, fill: None };
        let new = |shape| NewAnnotation { page: 0, shape, style: style.clone(), contents: String::new(), author: String::new() };
        let line = add_annotation(
            &mut doc,
            &new(Shape::Line { from: [100.0, 100.0], to: [300.0, 100.0], start: crate::LineEnding::None, end: crate::LineEnding::None }),
            &Meta::default(),
        )
        .unwrap();
        let box_ = add_annotation(&mut doc, &new(Shape::Rectangle { rect: [150.0, 50.0, 250.0, 150.0] }), &Meta::default()).unwrap();
        let art = art_from_drawing(&doc, 0, &[line, box_]).unwrap();
        // 200 wide across: the line spans the square less its margins, centred.
        assert!(art.starts_with("6 50 m 94 50 l S\n"), "{art}");
        assert!(art.contains(" re S\n"), "{art}");
        // Nothing drawable, or nothing at all: an error, not an empty symbol.
        assert!(art_from_drawing(&doc, 0, &[]).is_err());
        assert!(art_from_drawing(&doc, 0, &[99]).is_err());
        assert!(art_from_drawing(&doc, 5, &[0]).is_err());
    }

    #[test]
    fn symbols_are_placed_centred_labelled_and_listed() {
        let mut doc = blank();
        let layer = crate::layers::add(&mut doc, "Data", [0.0, 0.4, 0.9]).unwrap();
        let new = NewSymbol {
            page: 0,
            symbol: Symbol::DataJack,
            center: [300.0, 200.0],
            size: 20.0,
            label: "  PC (desk) \u{7} ".into(),
            color: [0.0, 0.4, 0.9],
            author: "Steve".into(),
            rotation: 0.0,
            custom: None,
        };
        let i = add_symbol(&mut doc, &new, &Meta::default()).unwrap();
        crate::layers::assign(&mut doc, 0, i, Some(layer)).unwrap();
        let ap = add_symbol(&mut doc, &NewSymbol { symbol: Symbol::WirelessAp, label: String::new(), ..new.clone() }, &Meta::default()).unwrap();
        let bytes = write_full(&doc, &SaveOptions::default()).unwrap();
        let doc = Document::open(Arc::new(bytes)).unwrap();
        let list = placed(&doc);
        // Saving renumbers objects: the layer is found again by its place in the list.
        let layer = crate::layers::list(&doc)[0].id;
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].symbol.as_str(), list[0].label.as_str(), list[0].layer), ("data-jack", "PC (desk)", Some(layer)));
        assert_eq!((list[1].index, list[1].symbol.as_str(), list[1].layer), (ap, "wireless-ap", None));
        // The unlabelled symbol is 20 points square and centred on the point.
        let p = page_refs(&doc).unwrap()[0];
        let entry = annots(&doc, p)[ap].clone();
        let d = doc.resolve(&entry).as_dict().cloned().unwrap();
        let r: Vec<f64> = d.get(b"Rect").and_then(Object::as_array).unwrap().iter().filter_map(Object::as_f64).collect();
        assert!((r[0] - 290.0).abs() < 1e-6 && (r[1] - 190.0).abs() < 1e-6 && (r[2] - 310.0).abs() < 1e-6 && (r[3] - 210.0).abs() < 1e-6, "{r:?}");
        assert!(d.get(b"AP").is_some(), "drawn for every viewer");
    }

    #[test]
    fn counts_group_by_symbol_label_and_layer() {
        let layer = crate::layers::LayerInfo {
            id: ObjRef::new(9, 0),
            name: "Data, 2nd floor".into(),
            color: [0.0; 3],
            locked: false,
            width: None,
            parent: None,
            depth: 0,
            role: None,
        };
        let at = |page, symbol: &str, label: &str, layer| Placed { page, index: 0, symbol: symbol.into(), label: label.into(), layer };
        let placed = [
            at(0, "data-jack", "PC", Some(layer.id)),
            at(1, "data-jack", "PC", Some(layer.id)),
            at(1, "data-jack", "PC", Some(layer.id)),
            at(1, "data-jack", "=VoIP", None),
            at(2, "mystery", "", None),
        ];
        let csv = counts_csv(&placed, &[layer]);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "Symbol,Label,Layer,Count,Pages");
        assert_eq!(lines[1], "Data jack (RJ-45),PC,\"Data, 2nd floor\",3,1 2");
        assert_eq!(lines[2], "Data jack (RJ-45),'=VoIP,,1,2");
        assert_eq!(lines[3], "mystery,,,1,3");
        assert_eq!(lines[4], "Total,,,5,");
        assert_eq!(lines.len(), 5 + 2 + placed.len());
    }

    #[test]
    fn symbols_turn_and_relabel_in_place() {
        let mut doc = blank();
        let new = NewSymbol {
            page: 0,
            symbol: Symbol::DataJack,
            center: [300.0, 200.0],
            size: 20.0,
            label: "PC".into(),
            color: [0.0, 0.4, 0.9],
            author: String::new(),
            rotation: 0.0,
            custom: None,
        };
        let i = add_symbol(&mut doc, &new, &Meta::default()).unwrap();
        let centre = |doc: &Document| {
            let p = page_refs(doc).unwrap()[0];
            let d = doc.resolve(&annots(doc, p)[i]).as_dict().unwrap().clone();
            let r: Vec<f64> = d.get(b"Rect").unwrap().as_array().unwrap().iter().filter_map(|x| x.as_f64()).collect();
            let label = d.get(b"PCLabel").unwrap().as_string().unwrap().to_text();
            let band = if label.is_empty() { 0.0 } else { LABEL_BAND };
            let k = 20.0 / BOX;
            ([(r[0] + r[2]) / 2.0, r[1] + (band + art_side(rotation_of(&d)) / 2.0) * k], label, rotation_of(&d))
        };
        edit_symbol(&mut doc, 0, i, Some("VoIP-3"), Some(90.0), &Meta::default()).unwrap();
        let (c, label, turn) = centre(&doc);
        assert!((c[0] - 300.0).abs() < 1e-6 && (c[1] - 200.0).abs() < 1e-6, "{c:?}");
        assert_eq!((label.as_str(), turn), ("VoIP-3", 90.0));
        assert_eq!(placed(&doc)[0].label, "VoIP-3");
        edit_symbol(&mut doc, 0, i, Some(""), Some(0.0), &Meta::default()).unwrap();
        let (c, label, turn) = centre(&doc);
        assert!((c[0] - 300.0).abs() < 1e-6 && (c[1] - 200.0).abs() < 1e-6, "{c:?}");
        assert_eq!((label.as_str(), turn), ("", 0.0));
        assert!(edit_symbol(&mut doc, 0, i, None, Some(f64::NAN), &Meta::default()).is_err());
    }

    #[test]
    fn bad_placements_are_refused() {
        let mut doc = blank();
        let base = NewSymbol {
            page: 0,
            symbol: Symbol::Rack,
            center: [10.0, 10.0],
            size: 20.0,
            label: String::new(),
            color: [0.0; 3],
            author: String::new(),
            rotation: 0.0,
            custom: None,
        };
        for bad in [
            NewSymbol { size: f64::NAN, ..base.clone() },
            NewSymbol { size: 0.0, ..base.clone() },
            NewSymbol { center: [f64::INFINITY, 0.0], ..base.clone() },
            NewSymbol { color: [1.5, 0.0, 0.0], ..base.clone() },
            NewSymbol { page: 9, ..base.clone() },
        ] {
            assert!(add_symbol(&mut doc, &bad, &Meta::default()).is_err(), "{bad:?}");
        }
        assert_eq!(clean_label(&"x".repeat(100)).len(), MAX_LABEL);
        // Your own symbol with no artwork is refused.
        let empty = CustomArt { name: "Mine".into(), art: String::new(), trade: None };
        assert!(add_symbol(&mut doc, &NewSymbol { custom: Some(empty), ..base.clone() }, &Meta::default()).is_err());
    }

    #[test]
    fn your_own_symbols_place_turn_relabel_and_count() {
        let mut doc = blank();
        let art = crate::svg_art::svg_art(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><path d="M1 1 L9 9 M1 9 L9 1" stroke="black"/></svg>"#,
        )
        .unwrap();
        let new = NewSymbol {
            page: 0,
            symbol: Symbol::DataJack,
            center: [200.0, 200.0],
            size: 24.0,
            label: "X-1".into(),
            color: [0.8, 0.1, 0.1],
            author: "Steve".into(),
            rotation: 0.0,
            custom: Some(CustomArt { name: "Duplex outlet".into(), art, trade: None }),
        };
        let i = add_symbol(&mut doc, &new, &Meta::default()).unwrap();
        edit_symbol(&mut doc, 0, i, Some("X-2"), Some(45.0), &Meta::default()).unwrap();
        let bytes = write_full(&doc, &SaveOptions::default()).unwrap();
        let doc = Document::open(Arc::new(bytes)).unwrap();
        let list = placed(&doc);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].symbol.as_str(), list[0].label.as_str()), ("custom:Duplex outlet", "X-2"));
        assert_eq!(display_name(&list[0].symbol), "Duplex outlet");
        assert_eq!(rotation_at(&doc, 0, i), 45.0);
    }
}
