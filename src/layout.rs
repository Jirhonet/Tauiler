pub type WindowId = usize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.width && p.y < self.y + self.height
    }
    pub fn center(self) -> Point {
        Point {
            x: self.x + self.width / 2,
            y: self.y + self.height / 2,
        }
    }
    pub fn near(self, other: Self, tolerance: i32) -> bool {
        (self.x - other.x).abs() <= tolerance
            && (self.y - other.y).abs() <= tolerance
            && (self.width - other.width).abs() <= tolerance
            && (self.height - other.height).abs() <= tolerance
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug)]
pub enum Node {
    Leaf(WindowId),
    Split {
        axis: Axis,
        ratio: f64,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    root: Option<Node>,
}

impl Layout {
    pub fn tiles(&self, area: Rect) -> Vec<(WindowId, Rect)> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            root.tiles(area, &mut out);
        }
        out
    }
    pub fn insert(&mut self, id: WindowId, area: Rect, cursor: Point, focused: Option<WindowId>) {
        let tiles = self.tiles(area);
        if tiles.iter().any(|(w, _)| *w == id) {
            return;
        }
        let target = tiles
            .iter()
            .find(|(_, r)| r.contains(cursor))
            .or_else(|| tiles.iter().find(|(w, _)| Some(*w) == focused))
            .or_else(|| tiles.first());
        if let Some((target, rect)) = target {
            let axis = if rect.width >= rect.height {
                Axis::Horizontal
            } else {
                Axis::Vertical
            };
            let before = match axis {
                Axis::Horizontal => cursor.x < rect.center().x,
                Axis::Vertical => cursor.y < rect.center().y,
            };
            if let Some(root) = &mut self.root {
                root.split(*target, id, axis, before);
            }
        } else {
            self.root = Some(Node::Leaf(id));
        }
    }
    pub fn remove(&mut self, id: WindowId) {
        self.root = self.root.take().and_then(|n| n.remove(id));
    }
    pub fn resize(&mut self, id: WindowId, dx: i32, dy: i32, area: Rect) {
        if let Some(root) = &mut self.root {
            // A diagonal drag can reach different ancestors for width and height.
            // Resolving both deltas together stopped after the first axis matched.
            if dx != 0 {
                root.resize(id, dx, 0, area);
            }
            if dy != 0 {
                root.resize(id, 0, dy, area);
            }
        }
    }
}

fn partition(area: Rect, axis: Axis, ratio: f64) -> (Rect, Rect) {
    let total = match axis {
        Axis::Horizontal => area.width,
        Axis::Vertical => area.height,
    };
    let size = ((total as f64 * ratio).round() as i32).clamp(0, total.max(0));
    match axis {
        Axis::Horizontal => (
            Rect {
                width: size,
                ..area
            },
            Rect {
                x: area.x + size,
                width: total - size,
                ..area
            },
        ),
        Axis::Vertical => (
            Rect {
                height: size,
                ..area
            },
            Rect {
                y: area.y + size,
                height: total - size,
                ..area
            },
        ),
    }
}

impl Node {
    fn tiles(&self, area: Rect, out: &mut Vec<(WindowId, Rect)>) {
        match self {
            Self::Leaf(w) => out.push((*w, area)),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (a, b) = partition(area, *axis, *ratio);
                first.tiles(a, out);
                second.tiles(b, out);
            }
        }
    }
    fn split(&mut self, target: WindowId, new: WindowId, axis: Axis, before: bool) {
        match self {
            Self::Leaf(w) if *w == target => {
                let (a, b) = if before { (new, *w) } else { (*w, new) };
                *self = Self::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(Self::Leaf(a)),
                    second: Box::new(Self::Leaf(b)),
                };
            }
            Self::Split { first, second, .. } => {
                first.split(target, new, axis, before);
                second.split(target, new, axis, before);
            }
            _ => {}
        }
    }
    fn remove(self, id: WindowId) -> Option<Self> {
        match self {
            Self::Leaf(w) => (w != id).then_some(Self::Leaf(w)),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => match (first.remove(id), second.remove(id)) {
                (Some(a), Some(b)) => Some(Self::Split {
                    axis,
                    ratio,
                    first: Box::new(a),
                    second: Box::new(b),
                }),
                (a, b) => a.or(b),
            },
        }
    }
    fn contains(&self, id: WindowId) -> bool {
        match self {
            Self::Leaf(w) => *w == id,
            Self::Split { first, second, .. } => first.contains(id) || second.contains(id),
        }
    }
    fn resize(&mut self, id: WindowId, dx: i32, dy: i32, area: Rect) -> bool {
        if let Self::Split {
            axis,
            ratio,
            first,
            second,
        } = self
        {
            let (a, b) = partition(area, *axis, *ratio);
            if first.resize(id, dx, dy, a) || second.resize(id, dx, dy, b) {
                return true;
            }
            if first.contains(id) || second.contains(id) {
                let (delta, size) = match axis {
                    Axis::Horizontal => (dx, area.width),
                    Axis::Vertical => (dy, area.height),
                };
                if delta != 0 && size > 0 {
                    *ratio = (*ratio + delta as f64 / size as f64).clamp(0.1, 0.9);
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn area() -> Rect {
        Rect {
            x: -1000,
            y: 0,
            width: 1001,
            height: 701,
        }
    }
    #[test]
    fn cursor_splits_only_target_and_removal_collapses() {
        let a = area();
        let mut l = Layout::default();
        l.insert(1, a, a.center(), None);
        l.insert(2, a, Point { x: 0, y: 350 }, None);
        let left = l.tiles(a)[0];
        l.insert(3, a, Point { x: 0, y: 650 }, None);
        assert_eq!(l.tiles(a)[0], left);
        assert_eq!(l.tiles(a).len(), 3);
        l.remove(3);
        assert_eq!(l.tiles(a)[0], left);
        assert_eq!(l.tiles(a)[1].1.height, 701);
    }
    #[test]
    fn many_insertions_cover_area_without_overlap() {
        let a = area();
        let mut l = Layout::default();
        for id in 1..30 {
            l.insert(
                id,
                a,
                Point {
                    x: -1000 + (id as i32 * 31) % 1001,
                    y: (id as i32 * 41) % 701,
                },
                None,
            );
        }
        let tiles = l.tiles(a);
        assert_eq!(
            tiles
                .iter()
                .map(|(_, r)| i64::from(r.width) * i64::from(r.height))
                .sum::<i64>(),
            i64::from(a.width) * i64::from(a.height)
        );
        for (i, (_, r)) in tiles.iter().enumerate() {
            for (_, s) in &tiles[i + 1..] {
                assert!(
                    r.x + r.width <= s.x
                        || s.x + s.width <= r.x
                        || r.y + r.height <= s.y
                        || s.y + s.height <= r.y
                );
            }
        }
        for id in 1..30 {
            l.remove(id);
        }
        assert!(l.tiles(a).is_empty());
    }
    #[test]
    fn resize_is_bounded_and_duplicate_insert_is_ignored() {
        let a = area();
        let mut l = Layout::default();
        l.insert(1, a, a.center(), None);
        l.insert(2, a, a.center(), None);
        l.insert(2, a, a.center(), None);
        l.resize(1, 100000, 0, a);
        let t = l.tiles(a);
        assert_eq!(t.len(), 2);
        assert!(t.iter().all(|(_, r)| r.width > 0));
    }
    #[test]
    fn diagonal_resize_updates_both_nearest_split_axes() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 1000,
            height: 800,
        };
        let mut l = Layout::default();
        l.insert(1, area, area.center(), None);
        l.insert(2, area, Point { x: 900, y: 400 }, None);
        l.insert(3, area, Point { x: 900, y: 700 }, None);
        l.resize(3, 100, 80, area);
        let tiles = l.tiles(area);
        assert_eq!(tiles.iter().find(|(id, _)| *id == 1).unwrap().1.width, 600);
        assert_eq!(tiles.iter().find(|(id, _)| *id == 2).unwrap().1.height, 480);
        assert_eq!(
            tiles.iter().find(|(id, _)| *id == 3).unwrap().1,
            Rect {
                x: 600,
                y: 480,
                width: 400,
                height: 320
            }
        );
    }
}
