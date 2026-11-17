//! The orders a task visits its chunks in (Chunky's `iterator/` and
//! `pattern` command). Each visits every chunk of the selection's bounding
//! box once (the shape is tested by the task), so `total` is the box's size
//! as in Chunky, and a task resumes by starting the same order `count`
//! chunks in.
use crate::selection::Selection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    /// 32x32-chunk regions, rings of regions outwards from the centre one;
    /// inside a region, rows (Chunky walks a full region along a Hilbert
    /// curve: the set of chunks is the same).
    Region,
    /// Square rings around the centre chunk, outwards.
    Concentric,
    /// Columns west to east, each north to south.
    Loop,
    /// One spiral from the centre chunk outwards.
    Spiral,
}

impl Pattern {
    pub const ALL: [Pattern; 4] = [
        Pattern::Concentric,
        Pattern::Loop,
        Pattern::Spiral,
        Pattern::Region,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Pattern::Region => "region",
            Pattern::Concentric => "concentric",
            Pattern::Loop => "loop",
            Pattern::Spiral => "spiral",
        }
    }
    pub fn parse(s: &str) -> Option<Pattern> {
        let s = s.to_ascii_lowercase();
        Self::ALL.into_iter().find(|p| p.name() == s)
    }
    pub fn names() -> String {
        Self::ALL.map(|p| p.name()).join(", ")
    }
}

/// The `i`-th cell of square rings around (0, 0): ring 0 is the centre, ring
/// `k` has `8k` cells from `(k, k-1)` down to `(k, -k)`, west to `(-k, -k)`,
/// south to `(-k, k)` and east to `(k, k)` (Chunky's concentric order).
fn ring_cell(i: u64) -> (i64, i64) {
    if i == 0 {
        return (0, 0);
    }
    // The ring k with (2k-1)^2 <= i < (2k+1)^2.
    let mut k = ((i as f64).sqrt() as u64).div_ceil(2).max(1);
    while (2 * k - 1) * (2 * k - 1) > i {
        k -= 1;
    }
    while (2 * k + 1) * (2 * k + 1) <= i {
        k += 1;
    }
    let j = i - (2 * k - 1) * (2 * k - 1);
    let side = 2 * k;
    let (seg, t) = ((j / side) as i64, (j % side) as i64);
    let k = k as i64;
    match seg {
        0 => (k, k - 1 - t),
        1 => (k - 1 - t, -k),
        2 => (-k, -k + 1 + t),
        _ => (-k + 1 + t, k),
    }
}

/// The chunks of a task, in its pattern's order.
pub struct ChunkIter {
    kind: Kind,
    /// How many chunks were handed out (or skipped by a resume).
    pub index: u64,
    pub total: u64,
}

enum Kind {
    Loop {
        x0: i32,
        z0: i32,
        dz: u64,
    },
    Concentric {
        cx: i32,
        cz: i32,
    },
    Spiral {
        x: i32,
        z: i32,
        /// The leg (0 east, 1 north, 2 west, 3 south), its length and the
        /// steps left on it.
        dir: u8,
        len: u32,
        left: u32,
    },
    Region {
        /// The box, inclusive.
        bx: (i32, i32),
        bz: (i32, i32),
        center: (i32, i32),
        regions: u64,
        region: u64,
        /// The current region's clipped cells and the next one's index.
        cells: (i32, i32, i32, i32),
        cell: u64,
    },
}

impl ChunkIter {
    /// The selection's chunks in its pattern's order, `skip` in. A rectangle
    /// or an ellipse always goes column by column (Chunky uses its strip
    /// order for them unless `loop` is asked for).
    pub fn new(sel: &Selection, skip: u64) -> Self {
        let (cx, cz) = (sel.center_chunk_x(), sel.center_chunk_z());
        let (rx, rz) = (sel.radius_chunks_x(), sel.radius_chunks_z());
        let total = (2 * rx as u64 + 1) * (2 * rz as u64 + 1);
        let pattern = if sel.shape.two_radii() {
            Pattern::Loop
        } else {
            sel.pattern
        };
        let kind = match pattern {
            Pattern::Loop => Kind::Loop {
                x0: cx - rx,
                z0: cz - rz,
                dz: 2 * rz as u64 + 1,
            },
            Pattern::Concentric => Kind::Concentric { cx, cz },
            Pattern::Spiral => Kind::Spiral {
                x: cx,
                z: cz,
                dir: 0,
                len: 1,
                left: 1,
            },
            Pattern::Region => {
                let rr = (rx as u32).div_ceil(32) as u64;
                Kind::Region {
                    bx: (cx - rx, cx + rx),
                    bz: (cz - rz, cz + rz),
                    center: (cx >> 5, cz >> 5),
                    regions: (2 * rr + 1) * (2 * rr + 1),
                    region: 0,
                    cells: (0, -1, 0, -1),
                    cell: 0,
                }
            }
        };
        let mut it = ChunkIter {
            kind,
            index: 0,
            total,
        };
        it.skip_to(skip.min(total));
        it
    }

    fn skip_to(&mut self, n: u64) {
        match &mut self.kind {
            Kind::Loop { .. } | Kind::Concentric { .. } => self.index = n,
            Kind::Region { .. } => {
                // Whole regions at a time, then cells into the last one.
                let mut left = n;
                while left > 0 {
                    let Kind::Region { cells, cell, .. } = &mut self.kind else {
                        unreachable!()
                    };
                    let size = region_size(*cells);
                    if *cell >= size {
                        if !self.next_region() {
                            break;
                        }
                        continue;
                    }
                    let take = left.min(size - *cell);
                    *cell += take;
                    self.index += take;
                    left -= take;
                }
            }
            Kind::Spiral { .. } => {
                for _ in 0..n {
                    self.next();
                }
            }
        }
    }

    /// The next region with cells in the box; false when none is left.
    fn next_region(&mut self) -> bool {
        let Kind::Region {
            bx,
            bz,
            center,
            regions,
            region,
            cells,
            cell,
        } = &mut self.kind
        else {
            return false;
        };
        while *region < *regions {
            let (dx, dz) = ring_cell(*region);
            *region += 1;
            let (rx, rz) = (center.0 + dx as i32, center.1 + dz as i32);
            let c = (
                (rx << 5).max(bx.0),
                ((rx << 5) + 31).min(bx.1),
                (rz << 5).max(bz.0),
                ((rz << 5) + 31).min(bz.1),
            );
            if c.0 <= c.1 && c.2 <= c.3 {
                *cells = c;
                *cell = 0;
                return true;
            }
        }
        false
    }

    /// How many chunks are left to hand out.
    pub fn left(&self) -> u64 {
        self.total - self.index
    }
}

fn region_size(c: (i32, i32, i32, i32)) -> u64 {
    if c.0 > c.1 || c.2 > c.3 {
        0
    } else {
        (c.1 - c.0 + 1) as u64 * (c.3 - c.2 + 1) as u64
    }
}

impl Iterator for ChunkIter {
    type Item = (i32, i32);

    fn next(&mut self) -> Option<(i32, i32)> {
        if self.index >= self.total {
            return None;
        }
        let i = self.index;
        let out = match &mut self.kind {
            Kind::Loop { x0, z0, dz } => (*x0 + (i / *dz) as i32, *z0 + (i % *dz) as i32),
            Kind::Concentric { cx, cz } => {
                let (dx, dz) = ring_cell(i);
                (*cx + dx as i32, *cz + dz as i32)
            }
            Kind::Spiral {
                x,
                z,
                dir,
                len,
                left,
            } => {
                let here = (*x, *z);
                match dir {
                    0 => *x += 1,
                    1 => *z -= 1,
                    2 => *x -= 1,
                    _ => *z += 1,
                }
                *left -= 1;
                if *left == 0 {
                    *dir = (*dir + 1) % 4;
                    if *dir % 2 == 0 {
                        *len += 1;
                    }
                    *left = *len;
                }
                here
            }
            Kind::Region { cells, cell, .. } => {
                if *cell >= region_size(*cells) {
                    if !self.next_region() {
                        return None;
                    }
                }
                let Kind::Region { cells, cell, .. } = &mut self.kind else {
                    unreachable!()
                };
                let h = (cells.3 - cells.2 + 1) as u64;
                let out = (cells.0 + (*cell / h) as i32, cells.2 + (*cell % h) as i32);
                *cell += 1;
                out
            }
        };
        self.index += 1;
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::ShapeKind;
    use std::collections::HashSet;

    fn sel(cx: f64, cz: f64, r: f64, shape: ShapeKind, pattern: Pattern) -> Selection {
        Selection {
            center_x: cx,
            center_z: cz,
            radius_x: r,
            radius_z: r * 0.5,
            shape,
            pattern,
            ..Selection::default()
        }
    }

    #[test]
    fn every_pattern_visits_the_box_once_and_resumes() {
        for &(cx, cz, r) in &[
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 16.0),
            (100.0, -300.0, 200.0),
            (-5000.0, 777.0, 1000.0),
            (511.0, 512.0, 530.0),
        ] {
            for shape in [ShapeKind::Square, ShapeKind::Circle, ShapeKind::Rectangle] {
                for p in Pattern::ALL {
                    let s = sel(cx, cz, r, shape, p);
                    let all: Vec<_> = ChunkIter::new(&s, 0).collect();
                    let it = ChunkIter::new(&s, 0);
                    assert_eq!(all.len() as u64, it.total, "{p:?} {shape:?} {r}");
                    let set: HashSet<_> = all.iter().copied().collect();
                    assert_eq!(set.len(), all.len(), "{p:?} twice");
                    let (x0, z0) = (s.center_chunk_x(), s.center_chunk_z());
                    let (rx, rz) = (s.radius_chunks_x(), s.radius_chunks_z());
                    assert!(set.iter().all(|&(x, z)| (x - x0).abs() <= rx && (z - z0).abs() <= rz));
                    let n = all.len() as u64;
                    for skip in [0, 1, 7, n / 3, n - 1, n] {
                        let skip = skip.min(n);
                        let rest: Vec<_> = ChunkIter::new(&s, skip).collect();
                        assert_eq!(&rest[..], &all[skip as usize..], "{p:?} skip {skip}");
                    }
                }
            }
        }
    }

    #[test]
    fn orders() {
        let s = sel(0.0, 0.0, 16.0, ShapeKind::Square, Pattern::Spiral);
        let v: Vec<_> = ChunkIter::new(&s, 0).collect();
        assert_eq!(v[0], (0, 0));
        assert_eq!(v[1], (1, 0));
        assert_eq!(*v.last().unwrap(), (1, 1));
        let s = sel(0.0, 0.0, 16.0, ShapeKind::Square, Pattern::Concentric);
        let v: Vec<_> = ChunkIter::new(&s, 0).collect();
        assert_eq!(&v[..3], &[(0, 0), (1, 0), (1, -1)]);
        assert_eq!(*v.last().unwrap(), (1, 1));
    }
}
