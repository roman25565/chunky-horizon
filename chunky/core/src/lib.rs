//! Chunky for Horizon, the parts that need no game: the selection and its
//! shapes, the orders chunks are visited in (patterns), the saved task files,
//! the command's input and the progress numbers. Ported from Chunky
//! (pop4959/Chunky, GPL-3.0), `common/src/main/java/org/popcraft/chunky`.
pub mod config;
pub mod input;
pub mod iter;
pub mod progress;
pub mod selection;
pub mod shape;
pub mod taskfile;

pub use iter::Pattern;
pub use selection::Selection;
pub use shape::{Shape, ShapeKind};
