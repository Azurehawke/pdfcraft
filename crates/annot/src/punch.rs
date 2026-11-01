//! RevuCraft's punch list and spaces.
//!
//! A punch item is a markup with a number, an assignee, a due date and an item status, kept in
//! the annotation as `/PCPunch << /No 3 /Assignee (…) /Due (2026-11-02) /Status /Open >>`. Its
//! number is drawn in a badge at the top right of the markup's appearance, so a printed sheet and
//! the list match, and other viewers show the badge too.
//!
//! A space is a named zone of a sheet (a room, a wing, a phase of work): a polygon or rectangle
//! comment marked `/PCSpace true`, its name in `/Contents`, drawn with a dashed outline, a light
//! fill and its name. A markup is in the space that holds the middle of its rectangle; when
//! spaces nest, the smallest wins.
//!
//! Item status is separate from a markup's review status (Accepted, Rejected…): it tracks the
//! work, the review status tracks the comment.

use pdfcraft_cos::{Dict, Document, Object, PdfString, Stream};
use pdfcraft_fonts::{literal, win_ansi};

use crate::{AnnotError, Meta, annot_ref, annots, n, page_ref, set_appearance};

pub const PUNCH: &[u8] = b"PCPunch";
pub const SPACE: &[u8] = b"PCSpace";
/// A markup's condition (`/PCCondition`): Existing, Proposed, Installed, Remove, or your own word.
pub const CONDITION: &[u8] = b"PCCondition";
/// The conditions RevuCraft draws a look for.
pub const CONDITIONS: [&str; 4] = ["Existing", "Proposed", "Installed", "Remove"];

/// The longest assignee, due date and space name kept (characters).
const MAX_TEXT: usize = 120;
/// The highest punch number kept (a file can claim anything).
pub const MAX_NUMBER: u32 = 999_999;
/// The badge's radius (points), and the space name's size.
const BADGE: f64 = 7.0;
pub const LABEL: f64 = 9.0;
/// The badge's and a space's colours.
const BADGE_COLOR: [f64; 3] = [0.82, 0.27, 0.12];
pub const SPACE_STROKE: [f64; 3] = [0.16, 0.52, 0.32];
pub const SPACE_FILL: [f64; 3] = [0.82, 0.94, 0.85];
/// Where the user moved a space's name (`/PCLabelAt [x y]`, page space). Absent, it sits inside
/// the shape.
pub const LABEL_AT: &[u8] = b"PCLabelAt";
/// Corners kept for the space test, at most.
const MAX_CORNERS: usize = 4096;

/// What state a punch item's work is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PunchStatus {
    #[default]
    Open,
    InProgress,
    Ready,
    Closed,
}

impl PunchStatus {
    pub const ALL: [PunchStatus; 4] = [PunchStatus::Open, PunchStatus::InProgress, PunchStatus::Ready, PunchStatus::Closed];

    /// The PDF name it is kept under.
    pub fn name(self) -> &'static str {
        match self {
            PunchStatus::Open => "Open",
            PunchStatus::InProgress => "InProgress",
            PunchStatus::Ready => "Ready",
            PunchStatus::Closed => "Closed",
        }
    }

    /// What a person reads (English; the interface translates it).
    pub fn label(self) -> &'static str {
        match self {
            PunchStatus::Open => "Open",
            PunchStatus::InProgress => "In progress",
            PunchStatus::Ready => "Ready for review",
            PunchStatus::Closed => "Closed",
        }
    }

    /// An unknown name reads as Open.
    pub fn from_name(name: &[u8]) -> Self {
        Self::ALL.into_iter().find(|s| s.name().as_bytes() == name).unwrap_or_default()
    }
}

/// A punch item's details.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Punch {
    pub no: u32,
    pub assignee: String,
    /// A date as typed (`2026-11-02`), or empty.
    pub due: String,
    pub status: PunchStatus,
    /// The badge isn't drawn (a closed item, with Hide closed items' badges on).
    pub badge_hidden: bool,
}

fn clean(s: &str) -> String {
    let kept: String = s.chars().filter(|c| !c.is_control()).collect();
    kept.trim().chars().take(MAX_TEXT).collect()
}

fn text(d: &Dict, key: &[u8]) -> String {
    clean(&d.get(key).and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default())
}

/// The punch item an annotation dictionary carries.
pub fn punch_of(d: &Dict) -> Option<Punch> {
    let p = d.get(PUNCH)?.as_dict()?;
    let no = p.get(b"No").and_then(Object::as_int).filter(|n| (1..=i64::from(MAX_NUMBER)).contains(n))?;
    Some(Punch {
        no: u32::try_from(no).ok()?,
        assignee: text(p, b"Assignee"),
        due: text(p, b"Due"),
        status: PunchStatus::from_name(p.name(b"Status").unwrap_or_default()),
        badge_hidden: matches!(p.get(b"NoBadge"), Some(Object::Bool(true))),
    })
}

/// A markup's condition, when it has one.
pub fn condition_of(d: &Dict) -> Option<String> {
    Some(text(d, CONDITION)).filter(|c| !c.is_empty())
}

/// Give the markup at `(page, index)` a condition (`None`: none). Installed closes its punch
/// item, if it has one. The markup is drawn again in the condition's look.
pub fn set_condition(doc: &mut Document, page: usize, index: usize, condition: Option<&str>, meta: &Meta) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    crate::unlocked(doc, r)?;
    let condition = condition.map(clean).filter(|c| !c.is_empty()).map(|c| c.chars().take(40).collect::<String>());
    let before = crate::annot_dict(doc, r);
    let mut punch = punch_of(&before);
    if condition.as_deref() == Some("Installed")
        && let Some(p) = punch.as_mut()
    {
        p.status = PunchStatus::Closed;
    }
    doc.update_dict(r, |d| {
        match &condition {
            Some(c) => d.set(CONDITION.to_vec(), PdfString::text(c)),
            None => {
                d.remove(CONDITION);
            }
        }
        if let Some(p) = &punch {
            d.set(PUNCH.to_vec(), Object::Dict(punch_dict(p)));
        }
        crate::touch(d, meta);
    })?;
    // The condition is kept either way; only its look depends on redrawing.
    let _ = set_appearance(doc, r);
    Ok(())
}

/// The conditions of the markups on `page`: (index, condition).
pub fn conditions(doc: &Document, page: usize) -> Vec<(usize, String)> {
    each(doc, page, condition_of)
}

fn punch_dict(p: &Punch) -> Dict {
    let mut v = Dict::new();
    v.set(b"No".to_vec(), Object::Int(i64::from(p.no)));
    let (assignee, due) = (clean(&p.assignee), clean(&p.due));
    if !assignee.is_empty() {
        v.set(b"Assignee".to_vec(), PdfString::text(&assignee));
    }
    if !due.is_empty() {
        v.set(b"Due".to_vec(), PdfString::text(&due));
    }
    v.set(b"Status".to_vec(), Object::name(p.status.name()));
    if p.badge_hidden {
        v.set(b"NoBadge".to_vec(), Object::Bool(true));
    }
    v
}

/// Put the markup at `(page, index)` on the punch list with `punch`'s details, or take it off
/// (`None`: the markup stays, its number goes). The badge is redrawn with the markup; a markup
/// whose look this program can't redraw keeps its item, without a badge.
pub fn set_punch(doc: &mut Document, page: usize, index: usize, punch: Option<&Punch>, meta: &Meta) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    crate::unlocked(doc, r)?;
    if let Some(p) = punch
        && !(1..=MAX_NUMBER).contains(&p.no)
    {
        return Err(AnnotError::Invalid("a punch item needs a number from 1".into()));
    }
    doc.update_dict(r, |d| {
        match punch {
            Some(p) => d.set(PUNCH.to_vec(), Object::Dict(punch_dict(p))),
            None => {
                d.remove(PUNCH);
            }
        }
        crate::touch(d, meta);
    })?;
    // The item is kept either way; only the badge depends on redrawing.
    let _ = set_appearance(doc, r);
    Ok(())
}

/// The name of the layer issue markups go on when they join the punch list.
pub const PUNCH_LAYER: &str = "Punch";

/// The punch items on `page`: (index, item).
pub fn punch_items(doc: &Document, page: usize) -> Vec<(usize, Punch)> {
    each(doc, page, punch_of)
}

/// Whether an annotation dictionary is a space.
pub fn is_space(d: &Dict) -> bool {
    matches!(d.get(SPACE), Some(Object::Bool(true))) && matches!(d.name(b"Subtype"), Some(b"Polygon" | b"Square"))
}

/// Make the polygon or rectangle at `(page, index)` a space called `name` (or rename it).
pub fn set_space(doc: &mut Document, page: usize, index: usize, name: &str, meta: &Meta) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    crate::unlocked(doc, r)?;
    let name = clean(name);
    if name.is_empty() {
        return Err(AnnotError::Invalid("a space needs a name".into()));
    }
    let before = crate::annot_dict(doc, r);
    if !matches!(before.name(b"Subtype"), Some(b"Polygon" | b"Square")) {
        return Err(AnnotError::Invalid("only a polygon or a rectangle can be a space".into()));
    }
    let fresh = !is_space(&before);
    doc.update_dict(r, |d| {
        d.set(SPACE.to_vec(), Object::Bool(true));
        d.set(b"Contents".to_vec(), PdfString::text(&name));
        d.set(b"Subj".to_vec(), PdfString::text("Space"));
        // A new space takes the space look; a renamed one keeps any restyling.
        if fresh {
            let col = |c: [f64; 3]| Object::Array(c.iter().map(|v| Object::Real(*v)).collect());
            d.set(b"C".to_vec(), col(SPACE_STROKE));
            d.set(b"IC".to_vec(), col(SPACE_FILL));
            d.set(b"CA".to_vec(), Object::Real(0.45));
            let mut bs = Dict::new();
            bs.set(b"W".to_vec(), Object::Real(1.0));
            bs.set(b"S".to_vec(), Object::name("D"));
            bs.set(b"D".to_vec(), Object::Array(vec![Object::Int(4), Object::Int(3)]));
            d.set(b"BS".to_vec(), Object::Dict(bs));
            d.remove(b"BE");
        }
        crate::touch(d, meta);
    })?;
    if let Err(e) = set_appearance(doc, r) {
        doc.set(r, Object::Dict(before));
        return Err(e);
    }
    Ok(())
}

/// The spaces on `page`: indexes.
pub fn spaces(doc: &Document, page: usize) -> Vec<usize> {
    each(doc, page, |d| is_space(d).then_some(())).into_iter().map(|(i, _)| i).collect()
}

fn each<T>(doc: &Document, page: usize, f: impl Fn(&Dict) -> Option<T>) -> Vec<(usize, T)> {
    let Ok(p) = page_ref(doc, page) else { return Vec::new() };
    annots(doc, p)
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let obj = doc.resolve(e);
            Some((i, f(obj.as_dict()?)?))
        })
        .collect()
}

/// Whether `p` is inside the polygon `corners` (even-odd).
pub fn contains(corners: &[[f64; 2]], p: [f64; 2]) -> bool {
    let corners = corners.get(..corners.len().min(MAX_CORNERS)).unwrap_or_default();
    let mut inside = false;
    let mut j = corners.len().wrapping_sub(1);
    for (i, a) in corners.iter().enumerate() {
        if let Some(b) = corners.get(j)
            && (a[1] > p[1]) != (b[1] > p[1])
        {
            let x = (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0];
            if p[0] < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// The area a polygon covers.
pub fn area(corners: &[[f64; 2]]) -> f64 {
    let corners = corners.get(..corners.len().min(MAX_CORNERS)).unwrap_or_default();
    let mut sum = 0.0;
    for (i, a) in corners.iter().enumerate() {
        if let Some(b) = corners.get((i + 1) % corners.len()) {
            sum += a[0] * b[1] - b[0] * a[1];
        }
    }
    (sum / 2.0).abs()
}

/// Of `spaces` (an id and its corners), the one holding `p`: the smallest when they nest.
pub fn space_at<T: Copy>(spaces: &[(T, Vec<[f64; 2]>)], p: [f64; 2]) -> Option<T> {
    spaces.iter().filter(|(_, c)| c.len() >= 3 && contains(c, p)).min_by(|a, b| area(&a.1).total_cmp(&area(&b.1))).map(|(id, _)| *id)
}

/// The corners of a space: a polygon's vertices, or a rectangle's four corners.
pub fn corners_of(d: &Dict) -> Vec<[f64; 2]> {
    let nums = |key: &[u8]| -> Vec<f64> {
        d.get(key).and_then(Object::as_array).map(|a| a.iter().filter_map(Object::as_f64).take(MAX_CORNERS * 2).collect()).unwrap_or_default()
    };
    if d.name(b"Subtype") == Some(b"Polygon") {
        return nums(b"Vertices").as_chunks::<2>().0.to_vec();
    }
    match nums(b"Rect").as_slice() {
        [a, b, c, e] => {
            let (x0, x1) = (a.min(*c), a.max(*c));
            let (y0, y1) = (b.min(*e), b.max(*e));
            vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
        }
        _ => Vec::new(),
    }
}

/// The distance from `p` to the segment `a`–`b`.
fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 > 0.0 { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let q = [a[0] + d[0] * t - p[0], a[1] + d[1] * t - p[1]];
    (q[0] * q[0] + q[1] * q[1]).sqrt()
}

/// Where a space's name sits when the user has not moved it: the point well inside the shape with
/// the most room round it (a grid search, so an L- or U-shaped space gets its middle arm), the
/// name's baseline-left point being set so the name is centred on it. `None` for no shape.
pub fn default_label(corners: &[[f64; 2]], size: f64, name_len: usize) -> Option<[f64; 2]> {
    let corners = corners.get(..corners.len().min(MAX_CORNERS))?;
    if corners.len() < 3 || corners.iter().any(|c| !c[0].is_finite() || !c[1].is_finite()) {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for c in corners {
        x0 = x0.min(c[0]);
        y0 = y0.min(c[1]);
        x1 = x1.max(c[0]);
        y1 = y1.max(c[1]);
    }
    const STEPS: usize = 24;
    let mut best: Option<([f64; 2], f64)> = None;
    for i in 0..=STEPS {
        for j in 0..=STEPS {
            let p = [x0 + (x1 - x0) * i as f64 / STEPS as f64, y0 + (y1 - y0) * j as f64 / STEPS as f64];
            if !contains(corners, p) {
                continue;
            }
            let mut room = f64::MAX;
            for (k, a) in corners.iter().enumerate() {
                if let Some(b) = corners.get((k + 1) % corners.len()) {
                    room = room.min(segment_distance(p, *a, *b));
                }
            }
            if best.is_none_or(|(_, r)| room > r) {
                best = Some((p, room));
            }
        }
    }
    let (p, _) = best?;
    let width = name_len.min(60) as f64 * size * 0.55;
    Some([p[0] - width / 2.0, p[1] - size / 2.0])
}

/// Where the user put a space's name (`/PCLabelAt`), when they have.
pub fn label_at_of(d: &Dict) -> Option<[f64; 2]> {
    let v: Vec<f64> = d.get(LABEL_AT)?.as_array()?.iter().filter_map(Object::as_f64).collect();
    match v.as_slice() {
        [x, y] if x.is_finite() && y.is_finite() => Some([*x, *y]),
        _ => None,
    }
}

/// Where a space's name is drawn: where the user put it, else [`default_label`].
pub fn label_point(d: &Dict, size: f64) -> Option<[f64; 2]> {
    label_at_of(d).or_else(|| default_label(&corners_of(d), size, text(d, b"Contents").chars().count()))
}

/// The point a space's name is drawn at, for the space at `(page, index)`; `None` when that
/// markup isn't a space.
pub fn space_label_at(doc: &Document, page: usize, index: usize) -> Option<[f64; 2]> {
    let p = page_ref(doc, page).ok()?;
    let entry = annots(doc, p).get(index).cloned()?;
    let obj = doc.resolve(&entry);
    let d = obj.as_dict()?;
    is_space(d).then(|| label_point(d, crate::text_size(d).unwrap_or(LABEL))).flatten()
}

/// Move a space's name to `at` (in page space), or back inside the space when `at` is `None`.
pub fn set_space_label(doc: &mut Document, page: usize, index: usize, at: Option<[f64; 2]>, meta: &Meta) -> Result<(), AnnotError> {
    if at.is_some_and(|p| !p.iter().all(|v| v.is_finite() && v.abs() <= 1e7)) {
        return Err(AnnotError::Invalid("invalid point".into()));
    }
    let (_, r) = annot_ref(doc, page, index)?;
    crate::unlocked(doc, r)?;
    let before = crate::annot_dict(doc, r);
    if !is_space(&before) {
        return Err(AnnotError::Invalid("only a space's name can be moved".into()));
    }
    doc.update_dict(r, |d| {
        match at {
            Some(p) => {
                d.set(LABEL_AT.to_vec(), Object::Array(p.iter().map(|v| Object::Real(*v)).collect()));
            }
            None => {
                d.remove(LABEL_AT);
            }
        }
        crate::touch(d, meta);
    })?;
    if let Err(e) = set_appearance(doc, r) {
        doc.set(r, Object::Dict(before));
        return Err(e);
    }
    Ok(())
}

/// The appearance with a punch item's badge or a space's name drawn over it (the original is
/// returned when there is neither, or its content can't be read).
pub(crate) fn decorate(d: &Dict, stream: Stream) -> Stream {
    let badge = punch_of(d).filter(|p| !p.badge_hidden).map(|p| p.no);
    let condition = condition_of(d).filter(|c| CONDITIONS.contains(&c.as_str()) && c != "Installed");
    let space = is_space(d).then(|| text(d, b"Contents")).filter(|s| !s.is_empty());
    if badge.is_none() && space.is_none() && condition.is_none() {
        return stream;
    }
    let Some(bbox) = stream.dict.get(b"BBox").and_then(Object::as_array).and_then(|a| {
        let v: Vec<f64> = a.iter().filter_map(Object::as_f64).collect();
        match v.as_slice() {
            [a, b, c, d] if [a, b, c, d].iter().all(|x| x.is_finite()) => Some([a.min(*c), b.min(*d), a.max(*c), b.max(*d)]),
            _ => None,
        }
    }) else {
        return stream;
    };
    let Ok(content) = stream.decoded() else { return stream };
    // Existing and Proposed are drawn faded (Proposed with a dashed box round it); Remove is
    // crossed out in red.
    let fade = match condition.as_deref() {
        Some("Existing") => Some(0.4),
        Some("Proposed") => Some(0.6),
        _ => None,
    };
    let mut out = b"q\n".to_vec();
    if fade.is_some() {
        out.extend_from_slice(b"/PCCond gs\n");
    }
    out.extend_from_slice(&content);
    out.extend_from_slice(b"\nQ\n");
    match condition.as_deref() {
        Some("Proposed") => out.extend(
            format!(
                "q 0.2 0.4 0.8 RG 0.75 w [3 2] 0 d {} {} {} {} re S Q\n",
                n(bbox[0] + 0.5),
                n(bbox[1] + 0.5),
                n((bbox[2] - bbox[0] - 1.0).max(0.0)),
                n((bbox[3] - bbox[1] - 1.0).max(0.0))
            )
            .bytes(),
        ),
        Some("Remove") => out.extend(
            format!(
                "q 0.85 0.1 0.1 RG 1.2 w 1 J {} {} m {} {} l {} {} m {} {} l S Q\n",
                n(bbox[0]),
                n(bbox[1]),
                n(bbox[2]),
                n(bbox[3]),
                n(bbox[0]),
                n(bbox[3]),
                n(bbox[2]),
                n(bbox[1])
            )
            .bytes(),
        ),
        _ => {}
    }
    if let Some(name) = space {
        let size = crate::text_size(d).unwrap_or(LABEL);
        // No shape to place it in: the old spot, at the top left of the box.
        let at = label_point(d, size).unwrap_or([bbox[0] + 4.0, bbox[3] - size - 3.0]);
        out.extend(space_label(&name, at, size));
    }
    if let Some(no) = badge {
        out.extend(badge_content(no, bbox));
    }
    let mut dict = stream.dict.clone();
    dict.remove(b"Filter");
    dict.remove(b"DecodeParms");
    dict.remove(b"Length");
    let mut res = dict.get(b"Resources").and_then(Object::as_dict).cloned().unwrap_or_default();
    let mut fonts = res.get(b"Font").and_then(Object::as_dict).cloned().unwrap_or_default();
    for (key, base) in [(&b"PCBadge"[..], "Helvetica-Bold"), (&b"PCLabel"[..], "Helvetica")] {
        let mut font = Dict::new();
        font.set(b"Type".to_vec(), Object::name("Font"));
        font.set(b"Subtype".to_vec(), Object::name("Type1"));
        font.set(b"BaseFont".to_vec(), Object::name(base));
        font.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
        fonts.set(key.to_vec(), Object::Dict(font));
    }
    res.set(b"Font".to_vec(), Object::Dict(fonts));
    if let Some(a) = fade {
        let mut gs = res.get(b"ExtGState").and_then(Object::as_dict).cloned().unwrap_or_default();
        let mut g = Dict::new();
        g.set(b"Type".to_vec(), Object::name("ExtGState"));
        g.set(b"CA".to_vec(), Object::Real(a));
        g.set(b"ca".to_vec(), Object::Real(a));
        gs.set(b"PCCond".to_vec(), Object::Dict(g));
        res.set(b"ExtGState".to_vec(), Object::Dict(gs));
    }
    dict.set(b"Resources".to_vec(), Object::Dict(res));
    Stream::flate(dict, &out)
}

/// A numbered circle in the box's top-right corner.
fn badge_content(no: u32, bbox: [f64; 4]) -> Vec<u8> {
    let (w, h) = (bbox[2] - bbox[0], bbox[3] - bbox[1]);
    let r = BADGE.min(w / 2.0).min(h / 2.0).max(3.0);
    let (x, y) = (bbox[2] - r, bbox[3] - r);
    let digits = no.to_string();
    // Helvetica's digits are all 0.556 em wide.
    let size = (r * 1.1).min(r * 1.7 / (0.556 * digits.len() as f64));
    let tw = 0.556 * size * digits.len() as f64;
    let k = r * 0.5523;
    let mut s = format!("q {} {} {} rg 1 1 1 RG 0.8 w\n", n(BADGE_COLOR[0]), n(BADGE_COLOR[1]), n(BADGE_COLOR[2]));
    s.push_str(&format!(
        "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c h B\n",
        n(x + r),
        n(y),
        n(x + r),
        n(y + k),
        n(x + k),
        n(y + r),
        n(x),
        n(y + r),
        n(x - k),
        n(y + r),
        n(x - r),
        n(y + k),
        n(x - r),
        n(y),
        n(x - r),
        n(y - k),
        n(x - k),
        n(y - r),
        n(x),
        n(y - r),
        n(x + k),
        n(y - r),
        n(x + r),
        n(y - k),
        n(x + r),
        n(y)
    ));
    s.push_str(&format!("1 1 1 rg BT /PCBadge {} Tf {} {} Td ({digits}) Tj ET Q\n", n(size), n(x - tw / 2.0), n(y - size * 0.36)));
    s.into_bytes()
}

/// A space's name, its baseline-left point at `at`.
fn space_label(name: &str, at: [f64; 2], size: f64) -> Vec<u8> {
    let name: String = name.chars().take(60).collect();
    let mut s = format!(
        "q {} {} {} rg BT /PCLabel {} Tf {} {} Td ",
        n(SPACE_STROKE[0] * 0.7),
        n(SPACE_STROKE[1] * 0.7),
        n(SPACE_STROKE[2] * 0.7),
        n(size),
        n(at[0]),
        n(at[1])
    )
    .into_bytes();
    s.extend(literal(&win_ansi(&name)));
    s.extend_from_slice(b" Tj ET Q\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_space_name_sits_inside_an_odd_shape() {
        // An L whose top-left corner is empty: the box's top-left is outside the shape.
        let l = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [40.0, 100.0], [40.0, 40.0], [0.0, 40.0]];
        assert!(!contains(&l, [4.0, 88.0]));
        let at = default_label(&l, 9.0, 7).unwrap_or([f64::NAN, f64::NAN]);
        let centre = [at[0] + 7.0 * 9.0 * 0.55 / 2.0, at[1] + 4.5];
        assert!(contains(&l, centre));
        // Fewer than three corners is no shape at all.
        assert_eq!(default_label(&l[..2], 9.0, 7), None);
    }

    #[test]
    fn punch_items_read_back_and_bad_numbers_are_not_items() {
        let mut d = Dict::new();
        assert_eq!(punch_of(&d), None);
        let mut p = Dict::new();
        p.set(b"No".to_vec(), Object::Int(3));
        p.set(b"Assignee".to_vec(), PdfString::text("Electrician"));
        p.set(b"Status".to_vec(), Object::name("Ready"));
        d.set(PUNCH.to_vec(), Object::Dict(p.clone()));
        assert_eq!(
            punch_of(&d),
            Some(Punch { no: 3, assignee: "Electrician".into(), due: String::new(), status: PunchStatus::Ready, badge_hidden: false })
        );
        p.set(b"No".to_vec(), Object::Int(0));
        d.set(PUNCH.to_vec(), Object::Dict(p.clone()));
        assert_eq!(punch_of(&d), None);
        p.set(b"No".to_vec(), Object::Int(i64::MAX));
        d.set(PUNCH.to_vec(), Object::Dict(p));
        assert_eq!(punch_of(&d), None);
        assert_eq!(PunchStatus::from_name(b"Nonsense"), PunchStatus::Open);
    }

    #[test]
    fn conditions_change_the_look_and_hidden_badges_are_not_drawn() {
        let mut sd = Dict::new();
        sd.set(b"BBox".to_vec(), Object::Array([0.0, 0.0, 40.0, 40.0].iter().map(|v| Object::Real(*v)).collect()));
        let look = |d: &Dict| String::from_utf8(decorate(d, Stream::flate(sd.clone(), b"0 0 m 40 40 l S")).decoded().unwrap()).unwrap();
        let mut d = Dict::new();
        d.set(CONDITION.to_vec(), PdfString::text("Existing"));
        assert!(look(&d).contains("/PCCond gs"), "{}", look(&d));
        d.set(CONDITION.to_vec(), PdfString::text("Remove"));
        assert!(look(&d).contains("0.85 0.1 0.1 RG"), "crossed out in red");
        d.set(CONDITION.to_vec(), PdfString::text("Installed"));
        assert_eq!(look(&d), "0 0 m 40 40 l S", "installed is drawn as it is");
        let mut p = Dict::new();
        p.set(b"No".to_vec(), Object::Int(4));
        p.set(b"NoBadge".to_vec(), Object::Bool(true));
        d.set(PUNCH.to_vec(), Object::Dict(p));
        assert!(!look(&d).contains("(4) Tj"), "a hidden badge isn't drawn");
    }

    #[test]
    fn a_markup_is_in_the_smallest_space_round_its_middle() {
        let wing = vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        let room = vec![[10.0, 10.0], [40.0, 10.0], [40.0, 40.0], [10.0, 40.0]];
        let spaces = vec![(1, wing.clone()), (2, room)];
        assert_eq!(space_at(&spaces, [20.0, 20.0]), Some(2));
        assert_eq!(space_at(&spaces, [70.0, 70.0]), Some(1));
        assert_eq!(space_at(&spaces, [170.0, 70.0]), None);
        assert_eq!(area(&wing), 10_000.0);
        // Too few corners, or none: nothing is inside.
        assert_eq!(space_at(&[(1, vec![[0.0, 0.0], [5.0, 5.0]])], [1.0, 1.0]), None);
        assert!(!contains(&[], [0.0, 0.0]));
        assert_eq!(area(&[]), 0.0);
    }

    #[test]
    fn the_badge_and_the_name_are_drawn_over_the_markup() {
        let mut d = Dict::new();
        let mut p = Dict::new();
        p.set(b"No".to_vec(), Object::Int(12));
        d.set(PUNCH.to_vec(), Object::Dict(p));
        let mut sd = Dict::new();
        sd.set(b"BBox".to_vec(), Object::Array([0.0, 0.0, 50.0, 30.0].iter().map(|v| Object::Real(*v)).collect()));
        let stream = Stream::flate(sd, b"0 0 m 50 30 l S");
        let out = decorate(&d, stream);
        let text = String::from_utf8(out.decoded().unwrap()).unwrap();
        assert!(text.starts_with("q\n0 0 m 50 30 l S\nQ\n"), "{text}");
        assert!(text.contains("(12) Tj"), "{text}");
        assert!(out.dict.get(b"Resources").and_then(Object::as_dict).and_then(|r| r.get(b"Font")).is_some());
    }
}
