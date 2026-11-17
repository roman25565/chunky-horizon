//! A task saved to disk (Chunky's `TaskLoader`): one `key=value` file per
//! world, `chunky/tasks/minecraft/overworld.properties` in the world's
//! folder. Pause and server stops write it; `continue` reads it back.
use crate::iter::Pattern;
use crate::selection::Selection;
use crate::shape::ShapeKind;

#[derive(Clone, Debug, PartialEq)]
pub struct SavedTask {
    pub selection: Selection,
    /// Who started it (a player's name, or `console`), and the player's
    /// UUID when it was one.
    pub owner: Option<String>,
    pub owner_uuid: Option<u128>,
    /// A cancelled or finished task can't be continued.
    pub cancelled: bool,
    /// The chunks of the pattern's order done so far, all of them: the task
    /// resumes this far in.
    pub chunks: u64,
    /// Time spent running, in milliseconds.
    pub time_ms: u64,
}

/// The file's path below the tasks folder: `minecraft/overworld.properties`.
pub fn file_name(world: &str) -> String {
    format!("{}.properties", world.replace(':', "/"))
}

impl SavedTask {
    pub fn write(&self) -> String {
        let s = &self.selection;
        let mut out = String::new();
        let mut line = |k: &str, v: String| {
            out.push_str(k);
            out.push('=');
            out.push_str(&v);
            out.push('\n');
        };
        line("world", s.world.clone());
        if let Some(o) = &self.owner {
            line("owner", o.clone());
        }
        if let Some(u) = self.owner_uuid {
            line("owner-uuid", format!("{u:032x}"));
        }
        line("cancelled", self.cancelled.to_string());
        line("center-x", s.center_x.to_string());
        line("center-z", s.center_z.to_string());
        line("radius", s.radius_x.to_string());
        if s.shape.two_radii() {
            line("radius-z", s.radius_z.to_string());
        }
        line("shape", s.shape.name().into());
        line("pattern", s.pattern.name().into());
        line("chunks", self.chunks.to_string());
        line("time", self.time_ms.to_string());
        out
    }

    /// Chunky's defaults for missing keys; None without a world.
    pub fn parse(text: &str) -> Option<SavedTask> {
        let get = |key: &str| {
            text.lines().find_map(|l| {
                let (k, v) = l.split_once('=')?;
                (k.trim() == key).then(|| v.trim().to_owned())
            })
        };
        let num = |key: &str, d: f64| get(key).and_then(|v| v.parse().ok()).unwrap_or(d);
        let radius = num("radius", 500.0);
        let selection = Selection {
            world: get("world")?,
            center_x: num("center-x", 0.0),
            center_z: num("center-z", 0.0),
            radius_x: radius,
            radius_z: num("radius-z", radius),
            shape: get("shape")
                .and_then(|s| ShapeKind::parse(&s))
                .unwrap_or(ShapeKind::Square),
            pattern: get("pattern")
                .and_then(|s| Pattern::parse(&s))
                .unwrap_or(Pattern::Region),
        };
        Some(SavedTask {
            selection,
            owner: get("owner"),
            owner_uuid: get("owner-uuid").and_then(|u| u128::from_str_radix(&u, 16).ok()),
            cancelled: get("cancelled").is_some_and(|v| v == "true"),
            chunks: get("chunks").and_then(|v| v.parse().ok()).unwrap_or(0),
            time_ms: get("time").and_then(|v| v.parse().ok()).unwrap_or(0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let t = SavedTask {
            selection: Selection {
                world: "minecraft:the_nether".into(),
                center_x: -12.5,
                center_z: 300.0,
                radius_x: 1000.0,
                radius_z: 250.0,
                shape: ShapeKind::Ellipse,
                pattern: Pattern::Spiral,
            },
            owner: Some("roman".into()),
            owner_uuid: Some(0x1234_5678_9abc_def0_1234_5678_9abc_def0),
            cancelled: false,
            chunks: 4242,
            time_ms: 99_000,
        };
        assert_eq!(SavedTask::parse(&t.write()), Some(t));
        assert_eq!(file_name("minecraft:overworld"), "minecraft/overworld.properties");
        let d = SavedTask::parse("world=minecraft:overworld\nradius=200\n").unwrap();
        assert_eq!(d.selection.radius_z, 200.0);
        assert!(!d.cancelled);
        assert!(SavedTask::parse("radius=1").is_none());
    }
}
