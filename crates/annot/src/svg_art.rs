//! RevuCraft's own symbols: an SVG drawing turned into symbol artwork (PDF path operators in the
//! library's 100-unit square), so it is placed, sized, turned, labelled and counted like the
//! built-in symbols.
//!
//! Symbols are line drawings, so the artwork takes the symbol's colour (its layer's): dark fills
//! and strokes are drawn in it, light ones (white panels behind a symbol) stay white. Gradients and
//! patterns are drawn in the symbol's colour; embedded pictures and text are left out (text in a
//! symbol should be outlines, or its label).
//!
//! The SVG is untrusted: its size, the number of path segments and the artwork's length are
//! capped, compressed SVG is refused, and a fault in the parser is caught.

use usvg::tiny_skia_path::PathSegment;

use crate::AnnotError;

/// The artwork's square (the library's).
const BOX: f64 = 100.0;
/// The art fills the square less this margin, so strokes on its edge stay inside.
const MARGIN: f64 = 4.0;
/// The largest SVG accepted (bytes).
pub const MAX_SVG: usize = 2 * 1024 * 1024;
/// Path segments drawn at most, and the artwork's length at most (bytes).
const MAX_SEGMENTS: usize = 40_000;
pub const MAX_ART: usize = 1024 * 1024;
/// Groups nested deeper than this are not followed.
const MAX_DEPTH: usize = 64;

/// One path in the SVG, flattened to its page coordinates.
struct Shape {
    segments: Vec<Seg>,
    fill: Option<(bool, bool)>, // (light, even-odd)
    stroke: Option<(bool, f64)>, // (light, width)
}

enum Seg {
    Move([f64; 2]),
    Line([f64; 2]),
    Cubic([f64; 2], [f64; 2], [f64; 2]),
    Close,
}

fn light(paint: &usvg::Paint) -> bool {
    match paint {
        usvg::Paint::Color(c) => {
            let luma = 0.299 * f64::from(c.red) + 0.587 * f64::from(c.green) + 0.114 * f64::from(c.blue);
            luma > 0.85 * 255.0
        }
        _ => false,
    }
}

fn collect(group: &usvg::Group, depth: usize, out: &mut Vec<Shape>, count: &mut usize) {
    if depth > MAX_DEPTH {
        return;
    }
    for node in group.children() {
        if *count >= MAX_SEGMENTS {
            return;
        }
        match node {
            usvg::Node::Group(g) => collect(g, depth + 1, out, count),
            usvg::Node::Path(p) => {
                if !p.is_visible() {
                    continue;
                }
                let t = p.abs_transform();
                let (sx, kx, ky, sy, tx, ty) = (f64::from(t.sx), f64::from(t.kx), f64::from(t.ky), f64::from(t.sy), f64::from(t.tx), f64::from(t.ty));
                let map = |pt: usvg::tiny_skia_path::Point| {
                    let (x, y) = (f64::from(pt.x), f64::from(pt.y));
                    [sx * x + kx * y + tx, ky * x + sy * y + ty]
                };
                let mut segments = Vec::new();
                let mut start = [0.0, 0.0];
                let mut last = [0.0, 0.0];
                for seg in p.data().segments() {
                    *count = count.saturating_add(1);
                    if *count >= MAX_SEGMENTS {
                        break;
                    }
                    match seg {
                        PathSegment::MoveTo(a) => {
                            let a = map(a);
                            segments.push(Seg::Move(a));
                            start = a;
                            last = a;
                        }
                        PathSegment::LineTo(a) => {
                            let a = map(a);
                            segments.push(Seg::Line(a));
                            last = a;
                        }
                        PathSegment::QuadTo(c, a) => {
                            let (c, a) = (map(c), map(a));
                            // A quadratic as the cubic that draws it.
                            let c1 = [last[0] + (c[0] - last[0]) * 2.0 / 3.0, last[1] + (c[1] - last[1]) * 2.0 / 3.0];
                            let c2 = [a[0] + (c[0] - a[0]) * 2.0 / 3.0, a[1] + (c[1] - a[1]) * 2.0 / 3.0];
                            segments.push(Seg::Cubic(c1, c2, a));
                            last = a;
                        }
                        PathSegment::CubicTo(c1, c2, a) => {
                            let a = map(a);
                            segments.push(Seg::Cubic(map(c1), map(c2), a));
                            last = a;
                        }
                        PathSegment::Close => {
                            segments.push(Seg::Close);
                            last = start;
                        }
                    }
                }
                let scale = (sx * sy - kx * ky).abs().sqrt();
                let fill = p.fill().map(|f| (light(f.paint()), matches!(f.rule(), usvg::FillRule::EvenOdd)));
                let stroke = p.stroke().map(|s| (light(s.paint()), f64::from(s.width().get()) * scale));
                if !segments.is_empty() && (fill.is_some() || stroke.is_some()) {
                    out.push(Shape { segments, fill, stroke });
                }
            }
            // Pictures and text are left out (see the module notes).
            usvg::Node::Image(_) | usvg::Node::Text(_) => {}
        }
    }
}

/// Turn an SVG into symbol artwork: path operators in the 100-unit square, drawn in the colour
/// already set (light parts in white).
pub fn svg_art(svg: &[u8]) -> Result<String, AnnotError> {
    if svg.is_empty() || svg.len() > MAX_SVG {
        return Err(AnnotError::Invalid("the SVG is empty or larger than 2 MB".into()));
    }
    if svg.starts_with(&[0x1f, 0x8b]) {
        return Err(AnnotError::Invalid("compressed SVG (.svgz) isn't supported; save it as plain SVG".into()));
    }
    let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| usvg::Tree::from_data(svg, &usvg::Options::default())));
    let tree = match parsed {
        Ok(Ok(tree)) => tree,
        Ok(Err(e)) => return Err(AnnotError::Invalid(format!("the SVG can't be read ({e})"))),
        Err(_) => return Err(AnnotError::Invalid("the SVG can't be read".into())),
    };
    let mut shapes = Vec::new();
    let mut count = 0usize;
    collect(tree.root(), 0, &mut shapes, &mut count);
    // The drawing's extent, so the art fills the square whatever the SVG's page size.
    let mut bounds: Option<[f64; 4]> = None;
    for s in &shapes {
        for seg in &s.segments {
            let pts: Vec<[f64; 2]> = match seg {
                Seg::Move(a) | Seg::Line(a) => vec![*a],
                Seg::Cubic(a, b, c) => vec![*a, *b, *c],
                Seg::Close => Vec::new(),
            };
            for p in pts.into_iter().filter(|p| p[0].is_finite() && p[1].is_finite()) {
                let b = bounds.get_or_insert([p[0], p[1], p[0], p[1]]);
                b[0] = b[0].min(p[0]);
                b[1] = b[1].min(p[1]);
                b[2] = b[2].max(p[0]);
                b[3] = b[3].max(p[1]);
            }
        }
    }
    let Some([x0, y0, x1, y1]) = bounds else {
        return Err(AnnotError::Invalid("the SVG has no lines or shapes to draw".into()));
    };
    let extent = (x1 - x0).max(y1 - y0);
    if !extent.is_finite() || extent <= 1e-9 {
        return Err(AnnotError::Invalid("the SVG's drawing has no size".into()));
    }
    let k = (BOX - 2.0 * MARGIN) / extent;
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    // SVG's y runs down the page, PDF's up.
    let at = |p: [f64; 2]| format!("{} {}", crate::n(BOX / 2.0 + (p[0] - cx) * k), crate::n(BOX / 2.0 - (p[1] - cy) * k));
    let mut art = String::new();
    for s in &shapes {
        if art.len() > MAX_ART {
            return Err(AnnotError::Invalid("the SVG is too detailed for a symbol".into()));
        }
        art.push_str("q\n");
        if let Some((true, _)) = s.fill {
            art.push_str("1 1 1 rg\n");
        }
        if let Some((white, width)) = s.stroke {
            if white {
                art.push_str("1 1 1 RG\n");
            }
            let w = if width.is_finite() { (width * k).clamp(0.5, 20.0) } else { 1.0 };
            art.push_str(&format!("{} w\n", crate::n(w)));
        }
        for seg in &s.segments {
            match seg {
                Seg::Move(a) => art.push_str(&format!("{} m\n", at(*a))),
                Seg::Line(a) => art.push_str(&format!("{} l\n", at(*a))),
                Seg::Cubic(a, b, c) => art.push_str(&format!("{} {} {} c\n", at(*a), at(*b), at(*c))),
                Seg::Close => art.push_str("h\n"),
            }
        }
        let op = match (s.fill, s.stroke.is_some()) {
            (Some((_, true)), true) => "B*",
            (Some((_, false)), true) => "B",
            (Some((_, true)), false) => "f*",
            (Some((_, false)), false) => "f",
            (None, _) => "S",
        };
        art.push_str(op);
        art.push_str("\nQ\n");
    }
    if art.len() > MAX_ART {
        return Err(AnnotError::Invalid("the SVG is too detailed for a symbol".into()));
    }
    Ok(art)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_svg_becomes_artwork_in_the_square() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
            <rect x="2" y="2" width="20" height="20" fill="white" stroke="black" stroke-width="2"/>
            <circle cx="12" cy="12" r="5" fill="#222"/>
            <path d="M2 12 Q 12 2 22 12" fill="none" stroke="#000"/>
        </svg>"##;
        let art = svg_art(svg).unwrap();
        assert!(art.contains("1 1 1 rg"), "the white panel stays white");
        assert!(art.contains(" c\n"), "curves are drawn");
        assert!(art.contains("\nB\n") && art.contains("\nS\n") && art.contains("\nf\n"), "{art}");
        assert_eq!(art.matches("q\n").count(), art.matches("Q\n").count());
        // Everything lands inside the square.
        for tok in art.split_whitespace().filter_map(|t| t.parse::<f64>().ok()) {
            assert!((-0.01..=100.01).contains(&tok), "{tok}");
        }
    }

    #[test]
    fn bad_svgs_are_refused_not_a_crash() {
        assert!(svg_art(b"").is_err());
        assert!(svg_art(b"not svg at all").is_err());
        assert!(svg_art(&[0x1f, 0x8b, 0, 0]).is_err());
        assert!(svg_art(br#"<svg xmlns="http://www.w3.org/2000/svg"><text>hi</text></svg>"#).is_err());
        assert!(svg_art(&vec![b' '; MAX_SVG + 1]).is_err());
    }
}
