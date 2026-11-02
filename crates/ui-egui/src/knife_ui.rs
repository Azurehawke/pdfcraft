//! RevuCraft's knife: a click on a plain smart wall cuts it across its thickness at that point.
//! Select the piece to take out and press Delete; the walls it was joined to let go. The engine
//! refuses walls with a gap or a door or window, and says so.

use pdfcraft_engine::Edit;
use pdfcraft_engine::walls::{self, Wall};

/// How far off a wall's outline a click still hits it, in points.
const REACH: f64 = 3.0;

/// The wall a click at `p` (user space) falls on and where along it (0 to 1), or `None`. When
/// walls overlap, the nearest centre line wins.
pub fn pick(walls: &[Wall], p: [f64; 2]) -> Option<(usize, f64)> {
    walls
        .iter()
        .filter_map(|w| {
            let ax = walls::axis(&w.quad)?;
            let t = ax.t_of(p);
            let d = [p[0] - ax.a[0], p[1] - ax.a[1]];
            let h = (d[0] * ax.left[0] + d[1] * ax.left[1]).abs();
            let reach = ax.thickness() / 2.0 + REACH;
            (t > 0.0 && t < 1.0 && h <= reach).then_some((h, w.index, t))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index, t)| (index, t))
}

/// The cut a click at `p` makes on `page`, if it is on a wall.
pub fn cut(walls: &[Wall], page: usize, p: [f64; 2]) -> Option<Edit> {
    let (index, t) = pick(walls, p)?;
    Some(Edit::SplitWall { page, index, t })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_engine::walls::WallData;

    fn wall(index: usize, quad: [[f64; 2]; 4]) -> Wall {
        Wall { index, nm: format!("w{index}"), quad, data: WallData::default(), locked: false }
    }

    // 100 long, 6 thick, centred on y = 0.
    fn plain() -> Wall {
        wall(3, [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]])
    }

    #[test]
    fn a_click_on_a_wall_cuts_it_where_it_was_clicked() {
        assert_eq!(pick(&[plain()], [40.0, 1.0]), Some((3, 0.4)));
        assert!(matches!(cut(&[plain()], 0, [40.0, 1.0]), Some(Edit::SplitWall { page: 0, index: 3, t }) if (t - 0.4).abs() < 1e-9));
    }

    #[test]
    fn a_click_off_the_wall_or_past_its_ends_is_not_a_cut() {
        let walls = [plain()];
        assert_eq!(pick(&walls, [40.0, 9.0]), None, "beyond the thickness and its reach");
        assert_eq!(pick(&walls, [-1.0, 0.0]), None, "beyond end A");
        assert_eq!(pick(&walls, [101.0, 0.0]), None, "beyond end B");
        assert_eq!(pick(&walls, [f64::NAN, 0.0]), None);
        assert!(cut(&walls, 0, [40.0, 9.0]).is_none());
    }

    #[test]
    fn a_click_just_outside_the_face_still_hits_within_reach() {
        assert_eq!(pick(&[plain()], [40.0, 5.5]), Some((3, 0.4)));
    }

    #[test]
    fn overlapping_walls_give_the_one_whose_centre_line_is_nearer() {
        let near = wall(1, [[0.0, 2.0], [100.0, 2.0], [100.0, -2.0], [0.0, -2.0]]);
        let far = wall(2, [[0.0, 6.0], [100.0, 6.0], [100.0, 4.0], [0.0, 4.0]]);
        assert_eq!(pick(&[far, near], [40.0, 1.0]).map(|(i, _)| i), Some(1));
    }
}
