//! Chunky's shapes (`shape/`), tested at a chunk's centre block as Chunky's
//! chunk-aligned shapes are: the centre is the centre chunk's middle
//! (`(cx << 4) + 8`) and a radius of `rc` chunks reaches `8 * (2 * rc + 1)`
//! blocks.
use crate::selection::Selection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Square,
    Circle,
    Rectangle,
    Ellipse,
    Diamond,
    Pentagon,
    Hexagon,
    Star,
    Triangle,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 9] = [
        ShapeKind::Circle,
        ShapeKind::Diamond,
        ShapeKind::Ellipse,
        ShapeKind::Hexagon,
        ShapeKind::Pentagon,
        ShapeKind::Rectangle,
        ShapeKind::Square,
        ShapeKind::Star,
        ShapeKind::Triangle,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ShapeKind::Square => "square",
            ShapeKind::Circle => "circle",
            ShapeKind::Rectangle => "rectangle",
            ShapeKind::Ellipse => "ellipse",
            ShapeKind::Diamond => "diamond",
            ShapeKind::Pentagon => "pentagon",
            ShapeKind::Hexagon => "hexagon",
            ShapeKind::Star => "star",
            ShapeKind::Triangle => "triangle",
        }
    }

    /// A shape's name as typed (`oval` is Chunky's other name for ellipse).
    pub fn parse(s: &str) -> Option<ShapeKind> {
        let s = s.to_ascii_lowercase();
        if s == "oval" {
            return Some(ShapeKind::Ellipse);
        }
        Self::ALL.into_iter().find(|k| k.name() == s)
    }

    pub fn names() -> String {
        Self::ALL.map(|k| k.name()).join(", ")
    }

    /// Whether the shape uses both radii (the others use the x one only).
    pub fn two_radii(self) -> bool {
        matches!(self, ShapeKind::Rectangle | ShapeKind::Ellipse)
    }
}

type P = (f64, f64);

/// `ShapeUtil.insideLine`: strictly left of a->b, so a point on an edge is out.
fn inside_line(a: P, b: P, c: P) -> bool {
    (b.0 - a.0) * (c.1 - a.1) > (b.1 - a.1) * (c.0 - a.0)
}

fn polygon_points(c: P, r: f64, degrees: &[f64]) -> Vec<P> {
    degrees
        .iter()
        .map(|d| {
            let t = d.to_radians();
            (c.0 + r * t.cos(), c.1 + r * t.sin())
        })
        .collect()
}

fn inside_polygon(points: &[P], p: P) -> bool {
    (0..points.len()).all(|i| inside_line(points[i], points[(i + 1) % points.len()], p))
}

/// A shape fixed to a selection, ready to test chunks.
#[derive(Clone, Debug)]
pub struct Shape {
    kind: ShapeKind,
    center: P,
    rx: f64,
    rz: f64,
    points: Vec<P>,
}

impl Shape {
    pub fn new(sel: &Selection) -> Self {
        let center = (
            f64::from((sel.center_chunk_x() << 4) + 8),
            f64::from((sel.center_chunk_z() << 4) + 8),
        );
        let rx = 8.0 * (2.0 * f64::from(sel.radius_chunks_x()) + 1.0);
        let rz = 8.0 * (2.0 * f64::from(sel.radius_chunks_z()) + 1.0);
        let (cx, cz) = center;
        let points = match sel.shape {
            ShapeKind::Diamond => vec![(cx, cz + rx), (cx - rx, cz), (cx, cz - rx), (cx + rx, cz)],
            ShapeKind::Triangle => vec![(cx + rx, cz + rx), (cx - rx, cz + rx), (cx, cz - rx)],
            ShapeKind::Pentagon | ShapeKind::Star => {
                polygon_points(center, rx, &[54.0, 126.0, 198.0, 270.0, 342.0])
            }
            ShapeKind::Hexagon => {
                polygon_points(center, rx, &[60.0, 120.0, 180.0, 240.0, 300.0, 360.0])
            }
            _ => Vec::new(),
        };
        Shape {
            kind: sel.shape,
            center,
            rx,
            rz,
            points,
        }
    }

    /// Whether the chunk `(x, z)` (chunk coordinates) is in the shape.
    pub fn contains_chunk(&self, x: i32, z: i32) -> bool {
        self.contains(f64::from(x) * 16.0 + 8.0, f64::from(z) * 16.0 + 8.0)
    }

    /// `Shape.isBounding(x, z)` for a block position.
    pub fn contains(&self, x: f64, z: f64) -> bool {
        let (cx, cz) = self.center;
        let p = (x, z);
        match self.kind {
            ShapeKind::Square => (x - cx).abs() <= self.rx && (z - cz).abs() <= self.rx,
            ShapeKind::Rectangle => (x - cx).abs() <= self.rx && (z - cz).abs() <= self.rz,
            ShapeKind::Circle => (cx - x).hypot(cz - z) <= self.rx,
            ShapeKind::Ellipse => {
                ((x - cx) / self.rx).powi(2) + ((z - cz) / self.rz).powi(2) <= 1.0
            }
            ShapeKind::Diamond | ShapeKind::Triangle | ShapeKind::Pentagon | ShapeKind::Hexagon => {
                inside_polygon(&self.points, p)
            }
            // Chunky's star: inside when four of the five long diagonals agree.
            ShapeKind::Star => {
                let q = &self.points;
                let h = |i: usize, j: usize| inside_line(q[i - 1], q[j - 1], p);
                let (h13, h24, h35, h41, h52) = (h(1, 3), h(2, 4), h(3, 5), h(4, 1), h(5, 2));
                (h13 && h24 && h35 && h41)
                    || (h24 && h35 && h41 && h52)
                    || (h35 && h41 && h52 && h13)
                    || (h41 && h52 && h13 && h24)
                    || (h52 && h13 && h24 && h35)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(shape: ShapeKind, r: f64) -> usize {
        let sel = Selection {
            shape,
            radius_x: r,
            radius_z: r,
            ..Selection::default()
        };
        let s = Shape::new(&sel);
        let rc = sel.radius_chunks_x();
        (-rc..=rc)
            .flat_map(|x| (-rc..=rc).map(move |z| (x, z)))
            .filter(|&(x, z)| s.contains_chunk(x, z))
            .count()
    }

    #[test]
    fn areas_fit_in_the_square() {
        let sq = count(ShapeKind::Square, 160.0);
        assert_eq!(sq, 21 * 21);
        for k in ShapeKind::ALL {
            let n = count(k, 160.0);
            assert!(n > 0 && n <= sq, "{k:?} {n}");
        }
        let c = count(ShapeKind::Circle, 1600.0) as f64;
        let ideal = std::f64::consts::PI * 100.5f64.powi(2);
        assert!((c / ideal - 1.0).abs() < 0.02, "{c} vs {ideal}");
        assert!(count(ShapeKind::Star, 1600.0) < count(ShapeKind::Pentagon, 1600.0));
        assert!(count(ShapeKind::Diamond, 1600.0) < count(ShapeKind::Square, 1600.0) * 6 / 10);
        assert!(count(ShapeKind::Triangle, 1600.0) < count(ShapeKind::Square, 1600.0) * 6 / 10);
    }

    #[test]
    fn names() {
        assert_eq!(ShapeKind::parse("OVAL"), Some(ShapeKind::Ellipse));
        assert_eq!(ShapeKind::parse("star"), Some(ShapeKind::Star));
        assert_eq!(ShapeKind::parse("blob"), None);
    }
}
