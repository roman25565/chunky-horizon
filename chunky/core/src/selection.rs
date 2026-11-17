//! Chunky's `Selection`: the world, centre, radii, shape and pattern the next
//! task takes. Centre and radii are in blocks.
use crate::iter::Pattern;
use crate::shape::ShapeKind;

#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    /// `minecraft:overworld`, `minecraft:the_nether` or `minecraft:the_end`.
    pub world: String,
    pub center_x: f64,
    pub center_z: f64,
    pub radius_x: f64,
    pub radius_z: f64,
    pub shape: ShapeKind,
    pub pattern: Pattern,
}

pub const DEFAULT_RADIUS: f64 = 500.0;
/// Chunky refuses centres and radii past 30,000,000 blocks.
pub const MAX_COORD: f64 = 30_000_000.0;

impl Default for Selection {
    fn default() -> Self {
        Selection {
            world: "minecraft:overworld".into(),
            center_x: 0.0,
            center_z: 0.0,
            radius_x: DEFAULT_RADIUS,
            radius_z: DEFAULT_RADIUS,
            shape: ShapeKind::Square,
            pattern: Pattern::Region,
        }
    }
}

impl Selection {
    /// `((int) centerX) >> 4`: Java's cast truncates toward zero first.
    pub fn center_chunk_x(&self) -> i32 {
        (self.center_x.trunc() as i32) >> 4
    }
    pub fn center_chunk_z(&self) -> i32 {
        (self.center_z.trunc() as i32) >> 4
    }
    pub fn radius_chunks_x(&self) -> i32 {
        (self.radius_x / 16.0).ceil() as i32
    }
    /// The z radius in chunks; the shapes with one radius use x for both.
    pub fn radius_chunks_z(&self) -> i32 {
        if self.shape.two_radii() {
            (self.radius_z / 16.0).ceil() as i32
        } else {
            self.radius_chunks_x()
        }
    }

    /// Chunky's `radius` command: square/rectangle and circle/ellipse follow
    /// whether the two radii are equal; the other shapes stay.
    pub fn set_radii(&mut self, rx: f64, rz: f64) {
        self.radius_x = rx;
        self.radius_z = rz;
        let equal = rx == rz;
        self.shape = match self.shape {
            ShapeKind::Square | ShapeKind::Rectangle => {
                if equal {
                    ShapeKind::Square
                } else {
                    ShapeKind::Rectangle
                }
            }
            ShapeKind::Circle | ShapeKind::Ellipse => {
                if equal {
                    ShapeKind::Circle
                } else {
                    ShapeKind::Ellipse
                }
            }
            other => other,
        };
    }
}
