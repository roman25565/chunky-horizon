// Chunky in the server's game, added as `game::chunky_glue`: the /chunky command, the
// running tasks and their files. The shapes, the orders, the files' format and the
// numbers live in the mod's crate, `chunky_core`.
//
// How a task generates: it holds a window of its chunks with Horizon's chunk tickets for
// mods (`native_force_chunk_in`, the same as `/forceload`), so the level's streaming
// generates, decorates and lights them and the server feeds its workers between ticks
// (`pump_chunks`). After the level's tick (`level_tick_post`) the task lets go of the
// chunks that are lit and post-processed (FULL, as `/pregen` waits for), which then
// unload and are saved like any chunk nobody is near, and holds the next ones. Only the
// window is ever loaded, whatever the radius.
//
// Everything runs on the server's thread. The state is behind a mutex taken with
// `try_lock`: a game call that came back into one of our hooks does nothing instead of
// waiting on itself.

use crate::game::{
    Game,
    native_server::{NativeCommand, NativeServerPlayer},
};
use chunky_core::{
    Pattern, Selection, Shape, ShapeKind,
    config::Config,
    input,
    iter::ChunkIter,
    progress::{self, Rate},
    taskfile::{self, SavedTask},
};
use horizon_client::text::{self, Text};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, TryLockError},
    time::{Duration, Instant},
};

/// The settings, from the server's folder.
const CONFIG: &str = "config/chunky/config.properties";
/// The tasks' folder in the world.
const TASKS: &str = "chunky/tasks";
/// The tickets' owner (the engine wants a namespace).
const TICKET: &str = "chunky:task";
/// A task that hasn't finished a chunk for this long says what it waits on.
const STALL: Duration = Duration::from_secs(60);
/// A running task's file is written this often (and when the world saves).
const AUTOSAVE: Duration = Duration::from_secs(60);
/// A `confirm` must come within this long.
const CONFIRM: Duration = Duration::from_secs(60);
const WORLDS: [&str; 3] = [
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
];

struct Slot {
    key: (i32, i32),
    /// Held with a ticket until done.
    forced: bool,
    done: bool,
}

struct Task {
    sel: Selection,
    shape: Shape,
    iter: ChunkIter,
    /// The chunks handed out and not yet counted, in the order's order.
    window: VecDeque<Slot>,
    forced: usize,
    /// Chunks of the order done, all of those before it: what a resume skips.
    done: u64,
    owner: Option<String>,
    owner_uuid: Option<u128>,
    /// Time run before this start, and when this run began.
    before_ms: u64,
    started: Instant,
    rate: Rate,
    current: (i32, i32),
    last_update: Instant,
    last_progress: Instant,
    last_save: Instant,
    /// A ticket on the centre chunk that opens a level nobody is in, until the
    /// task's first look at it.
    keepalive: Option<(i32, i32)>,
}

impl Task {
    fn time_ms(&self) -> u64 {
        self.before_ms + self.started.elapsed().as_millis() as u64
    }
    fn saved(&self, cancelled: bool) -> SavedTask {
        SavedTask {
            selection: self.sel.clone(),
            owner: self.owner.clone(),
            owner_uuid: self.owner_uuid,
            cancelled,
            chunks: self.done,
            time_ms: self.time_ms(),
        }
    }
}

enum Pending {
    /// Start anew over a saved task.
    Start(SavedTask),
    /// Cancel one world's task, or all.
    Cancel(Option<String>),
}

struct State {
    config: Config,
    selection: Selection,
    tasks: HashMap<String, Task>,
    pending: HashMap<String, (Pending, Instant)>,
    world: Option<PathBuf>,
    /// The saved tasks were looked at once the world was open.
    booted: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn load_config() -> Config {
    let path = Path::new(CONFIG);
    match std::fs::read_to_string(path) {
        Ok(text) => Config::parse(&text),
        Err(_) => {
            let c = Config::default();
            save_config(&c);
            c
        }
    }
}

fn save_config(c: &Config) {
    let path = Path::new(CONFIG);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, c.write()) {
        log::warn!("Chunky: can't write {CONFIG}: {e}");
    }
}

fn state() -> Option<MutexGuard<'static, Option<State>>> {
    match STATE.try_lock() {
        Ok(g) => Some(g),
        Err(TryLockError::Poisoned(e)) => Some(e.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

fn init(slot: &mut Option<State>) -> &mut State {
    slot.get_or_insert_with(|| State {
        config: load_config(),
        selection: Selection::default(),
        tasks: HashMap::new(),
        pending: HashMap::new(),
        world: None,
        booted: false,
    })
}

// ---- Messages ------------------------------------------------------------------------------

fn prefixed(msg: &str, color: Option<u32>) -> Text {
    let body = Text::literal(msg.to_owned());
    Text::literal("[Chunky] ")
        .with_color(text::GREEN)
        .append(match color {
            Some(c) => body.with_color(c),
            None => body,
        })
}

fn say(game: &mut Game<'_>, msg: &str) {
    game.native_feedback(prefixed(msg, None));
}

fn fail(game: &mut Game<'_>, msg: &str) {
    game.native_feedback(prefixed(msg, Some(text::RED)));
}

/// A task's line: the server's log, and the player who started it.
fn report(game: &mut Game<'_>, owner: Option<u128>, msg: &str) {
    let line = format!("[Chunky] {msg}");
    if game.options.dedicated.is_some() {
        crate::game::dedicated::log("INFO", &line);
    } else {
        log::info!("{line}");
    }
    if let Some(uuid) = owner {
        game.native_tell(uuid, prefixed(msg, None));
    }
}

// ---- Files ---------------------------------------------------------------------------------

fn task_path(world_dir: &Path, world: &str) -> PathBuf {
    world_dir.join(TASKS).join(taskfile::file_name(world))
}

fn read_task(st: &State, world: &str) -> Option<SavedTask> {
    let path = task_path(st.world.as_ref()?, world);
    SavedTask::parse(&std::fs::read_to_string(path).ok()?)
}

fn write_task(world_dir: Option<&PathBuf>, task: &SavedTask) {
    let Some(dir) = world_dir else {
        return;
    };
    let path = task_path(dir, &task.selection.world);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, task.write()) {
        log::warn!("Chunky: can't write {}: {e}", path.display());
    }
}

// ---- Commands ------------------------------------------------------------------------------

const USAGES: &[&str] = &[
    "chunky",
    "chunky help",
    "chunky start [world:word] [shape:word] [x:number] [z:number] [radius:word] [radiusz:word]",
    "chunky center [x:number] [z:number]",
    "chunky radius <radius:word> [radiusz:word]",
    "chunky corners <x1:number> <z1:number> <x2:number> <z2:number>",
    "chunky shape <shape:word>",
    "chunky pattern <pattern:word>",
    "chunky world [world:word]",
    "chunky spawn",
    "chunky worldborder [world:word]",
    "chunky pause [world:word]",
    "chunky continue [world:word]",
    "chunky cancel [world:word]",
    "chunky confirm",
    "chunky progress",
    "chunky quiet <seconds:integer>",
    "chunky silent",
    "chunky selection",
    "chunky reload [what:word]",
    "chunky trim [arguments:text]",
];

const HELP: &[&str] = &[
    "chunky start [world] [shape] [x z] [radius] [radiusZ] - start a task",
    "chunky pause | continue | cancel [world] - pause (saved), go on, stop for good",
    "chunky confirm - confirm a cancel or a new start",
    "chunky progress - show the running tasks",
    "chunky center <x> <z> | spawn | worldborder [world] - set the centre",
    "chunky radius <r> [rz] - radius in blocks; 20c chunks, 2r regions, 1k thousands, +/- relative",
    "chunky corners <x1> <z1> <x2> <z2> - centre and radii from two corners",
    "chunky shape <shape> - square, circle, rectangle, ellipse, diamond, pentagon, hexagon, star, triangle",
    "chunky pattern <pattern> - region, concentric, loop, spiral",
    "chunky world [world] - overworld, the_nether, the_end",
    "chunky selection | quiet <s> | silent | reload",
];

pub fn commands(commands: &mut Vec<NativeCommand>) {
    let level = load_config().permission;
    for usage in USAGES {
        commands.push(if level == 0 {
            NativeCommand::new(usage)
        } else {
            NativeCommand::op(usage, level)
        });
    }
}

/// Who runs a command: a player, or the console.
struct Sender {
    name: String,
    uuid: Option<u128>,
    position: Option<[f64; 3]>,
    dimension: Option<String>,
}

pub(in crate::game) fn perform(
    game: &mut Game<'_>,
    player: &NativeServerPlayer,
    line: &str,
    ran: &mut bool,
) {
    let sender = Sender {
        name: player.name.clone(),
        uuid: Some(player.uuid),
        position: Some(player.position),
        dimension: Some(player.dimension.clone()),
    };
    run(game, sender, line, ran);
}

pub(in crate::game) fn console(game: &mut Game<'_>, line: &str, ran: &mut bool) {
    let sender = Sender {
        name: "console".into(),
        uuid: None,
        position: None,
        dimension: None,
    };
    run(game, sender, line, ran);
}

fn run(game: &mut Game<'_>, sender: Sender, line: &str, ran: &mut bool) {
    let words: Vec<&str> = line.split_whitespace().collect();
    if words.first() != Some(&"chunky") {
        return;
    }
    *ran = true;
    let Some(mut guard) = state() else {
        return;
    };
    let st = init(&mut *guard);
    if st.world.is_none() {
        st.world = game.native_world_dir();
    }
    let sub = words.get(1).copied().unwrap_or("help").to_ascii_lowercase();
    let args = &words[2.min(words.len())..];
    match sub.as_str() {
        "help" => {
            for line in HELP {
                say(game, line);
            }
        }
        "start" => cmd_start(game, st, &sender, args),
        "center" => {
            let (x, z) = match args {
                [] => match sender.position {
                    Some(p) => (p[0], p[2]),
                    None => return fail(game, "Usage: chunky center <x> <z>"),
                },
                [x, z, ..] => match (input::coordinate(x), input::coordinate(z)) {
                    (Some(x), Some(z)) => (x, z),
                    _ => return fail(game, "Invalid coordinates."),
                },
                _ => return fail(game, "Usage: chunky center <x> <z>"),
            };
            st.selection.center_x = x;
            st.selection.center_z = z;
            say(
                game,
                &format!(
                    "Center changed to {}, {}.",
                    input::fmt_number(x),
                    input::fmt_number(z)
                ),
            );
        }
        "radius" => {
            let s = &mut st.selection;
            let Some(rx) = args.first().and_then(|a| input::radius(a, s.radius_x)) else {
                return fail(game, "Usage: chunky radius <radius> [radiusZ] (0 to 30000000)");
            };
            let rz = match args.get(1) {
                Some(a) => match input::radius(a, s.radius_z) {
                    Some(v) => v,
                    None => return fail(game, "Invalid z radius."),
                },
                None => rx,
            };
            s.set_radii(rx, rz);
            let msg = if rx == rz {
                format!("Radius changed to {}.", input::fmt_number(rx))
            } else {
                format!(
                    "Radius changed for x to {}, and for z to {}.",
                    input::fmt_number(rx),
                    input::fmt_number(rz)
                )
            };
            say(game, &msg);
        }
        "corners" => {
            let nums: Vec<f64> = args.iter().filter_map(|a| input::coordinate(a)).collect();
            if args.len() != 4 || nums.len() != 4 {
                return fail(game, "Usage: chunky corners <x1> <z1> <x2> <z2>");
            }
            let s = &mut st.selection;
            s.center_x = (nums[0] + nums[2]) / 2.0;
            s.center_z = (nums[1] + nums[3]) / 2.0;
            let (rx, rz) = ((nums[0] - nums[2]).abs() / 2.0, (nums[1] - nums[3]).abs() / 2.0);
            s.radius_x = rx;
            s.radius_z = rz;
            s.shape = if rx == rz {
                ShapeKind::Square
            } else {
                ShapeKind::Rectangle
            };
            let (cx, cz) = (s.center_x, s.center_z);
            say(
                game,
                &format!(
                    "Center changed to {}, {}.",
                    input::fmt_number(cx),
                    input::fmt_number(cz)
                ),
            );
            say(
                game,
                &format!(
                    "Radius changed for x to {}, and for z to {}.",
                    input::fmt_number(rx),
                    input::fmt_number(rz)
                ),
            );
        }
        "shape" => match args.first().and_then(|a| ShapeKind::parse(a)) {
            Some(k) => {
                st.selection.shape = k;
                say(game, &format!("Shape changed to {}.", k.name()));
            }
            None => fail(
                game,
                &format!(
                    "Shape {} not found. Available shapes: {}",
                    args.first().unwrap_or(&""),
                    ShapeKind::names()
                ),
            ),
        },
        "pattern" => match args.first().and_then(|a| Pattern::parse(a)) {
            Some(p) => {
                st.selection.pattern = p;
                say(game, &format!("Pattern changed to {}.", p.name()));
            }
            None => fail(
                game,
                &format!(
                    "Pattern {} not found. Available patterns: {}",
                    args.first().unwrap_or(&""),
                    Pattern::names()
                ),
            ),
        },
        "world" => {
            let w = match args.first() {
                Some(a) => input::world(a).map(str::to_owned),
                None => sender.dimension.clone(),
            };
            match w {
                Some(w) => {
                    say(game, &format!("World changed to {w}."));
                    st.selection.world = w;
                }
                None => fail(
                    game,
                    &format!(
                        "World {} not found. Available worlds: {}",
                        args.first().unwrap_or(&""),
                        input::WORLDS
                    ),
                ),
            }
        }
        "spawn" => {
            let p = game.server_position();
            st.selection.center_x = p[0];
            st.selection.center_z = p[2];
            say(
                game,
                &format!(
                    "Center changed to {}, {}.",
                    input::fmt_number(p[0]),
                    input::fmt_number(p[2])
                ),
            );
        }
        "worldborder" => {
            let world = match args.first() {
                Some(a) => match input::world(a) {
                    Some(w) => w.to_owned(),
                    None => return fail(game, &format!("World {a} not found.")),
                },
                None => st.selection.world.clone(),
            };
            let Some([x0, z0, x1, z1]) = game.native_world_border(&world) else {
                return fail(game, "This world has no border to read.");
            };
            let s = &mut st.selection;
            s.world = world;
            s.center_x = (x0 + x1) / 2.0;
            s.center_z = (z0 + z1) / 2.0;
            let (rx, rz) = ((x1 - x0) / 2.0, (z1 - z0) / 2.0);
            s.set_radii(rx.min(30_000_000.0), rz.min(30_000_000.0));
            let (cx, cz, rx) = (s.center_x, s.center_z, s.radius_x);
            say(
                game,
                &format!(
                    "Center changed to {}, {}.",
                    input::fmt_number(cx),
                    input::fmt_number(cz)
                ),
            );
            say(game, &format!("Radius changed to {}.", input::fmt_number(rx)));
            if rx >= 1_000_000.0 {
                fail(
                    game,
                    "The border is the default one (about 30 million blocks): set a smaller border or radius before starting.",
                );
            }
        }
        "pause" => {
            let worlds = target_worlds(st, args);
            let mut any = false;
            for w in worlds {
                if let Some(task) = st.tasks.remove(&w) {
                    stop(game, &task);
                    write_task(st.world.as_ref(), &task.saved(false));
                    say(game, &format!("Task paused for {w}."));
                    any = true;
                }
            }
            if !any {
                fail(game, "No tasks to pause.");
            }
        }
        "continue" => {
            let worlds = target_worlds(st, args);
            let mut any = false;
            for w in worlds {
                if st.tasks.contains_key(&w) {
                    continue;
                }
                if let Some(saved) = read_task(st, &w).filter(|t| !t.cancelled) {
                    start_task(game, st, saved);
                    say(game, &format!("Task continuing for {w}."));
                    any = true;
                }
            }
            if !any {
                fail(game, "No tasks to continue.");
            }
        }
        "cancel" => {
            let world = match args.first() {
                Some(a) => match input::world(a) {
                    Some(w) => Some(w.to_owned()),
                    None => return fail(game, &format!("World {a} not found.")),
                },
                None => None,
            };
            st.pending
                .insert(sender.name.clone(), (Pending::Cancel(world), Instant::now()));
            say(
                game,
                "Cancelled tasks cannot be continued. If you are sure you want to cancel, type '/chunky confirm'.",
            );
        }
        "confirm" => {
            let pending = st
                .pending
                .remove(&sender.name)
                .filter(|(_, at)| at.elapsed() <= CONFIRM);
            match pending {
                None => fail(game, "Nothing to confirm!"),
                Some((Pending::Start(saved), _)) => {
                    let w = saved.selection.world.clone();
                    if st.tasks.contains_key(&w) {
                        return fail(game, &format!("Task already started for {w}!"));
                    }
                    announce_start(game, &saved.selection);
                    start_task(game, st, saved);
                }
                Some((Pending::Cancel(world), _)) => cancel(game, st, world),
            }
        }
        "progress" => {
            if st.tasks.is_empty() {
                return fail(game, "No tasks running.");
            }
            for w in WORLDS {
                if let Some(t) = st.tasks.get_mut(w) {
                    let rate = t.rate.rate(now_ms(t));
                    let line = progress::update_line(w, t.done, t.iter.total, rate, t.current);
                    say(game, &line);
                }
            }
        }
        "quiet" => match args.first().and_then(|a| a.parse::<i64>().ok()) {
            Some(n) => {
                st.config.update_interval = n.max(0) as u64;
                save_config(&st.config);
                say(
                    game,
                    &format!("Quiet interval set to {} seconds.", st.config.update_interval),
                );
            }
            None => fail(game, "Usage: chunky quiet <seconds>"),
        },
        "silent" => {
            st.config.silent = !st.config.silent;
            save_config(&st.config);
            let on = if st.config.silent { "enabled" } else { "disabled" };
            say(game, &format!("Silent mode {on}."));
        }
        "selection" => {
            let s = st.selection.clone();
            say(game, "Current selection");
            say(game, &format!("World: {}", s.world));
            say(game, &format!("Shape: {}", s.shape.name()));
            say(game, &format!("Pattern: {}", s.pattern.name()));
            say(
                game,
                &format!(
                    "Center: {}, {}",
                    input::fmt_number(s.center_x),
                    input::fmt_number(s.center_z)
                ),
            );
            if s.shape.two_radii() {
                say(
                    game,
                    &format!(
                        "Radii: {}, {}",
                        input::fmt_number(s.radius_x),
                        input::fmt_number(s.radius_z)
                    ),
                );
            } else {
                say(game, &format!("Radius: {}", input::fmt_number(s.radius_x)));
            }
        }
        "reload" => {
            if args.first().is_some_and(|a| a.eq_ignore_ascii_case("tasks")) && !st.tasks.is_empty()
            {
                return fail(
                    game,
                    "The configuration cannot be reloaded while tasks are running.",
                );
            }
            st.config = load_config();
            say(game, "Successfully reloaded configuration.");
        }
        "trim" => fail(
            game,
            "Trim is not available on Horizon: the engine gives mods no safe way to delete saved chunks while the server runs.",
        ),
        other => fail(
            game,
            &format!("Unknown subcommand '{other}'. Type '/chunky help'."),
        ),
    }
}

/// The worlds `pause`/`continue` act on: the one named, else all three.
fn target_worlds(_st: &State, args: &[&str]) -> Vec<String> {
    match args.first().and_then(|a| input::world(a)) {
        Some(w) => vec![w.to_owned()],
        None => WORLDS.iter().map(|w| (*w).to_owned()).collect(),
    }
}

fn cmd_start(game: &mut Game<'_>, st: &mut State, sender: &Sender, args: &[&str]) {
    // start [world [shape [x z [radius [radiusZ]]]]]: each argument changes the selection.
    let mut sel = st.selection.clone();
    if let Some(a) = args.first() {
        match input::world(a) {
            Some(w) => sel.world = w.to_owned(),
            None => {
                return fail(
                    game,
                    &format!("World {a} not found. Available worlds: {}", input::WORLDS),
                );
            }
        }
    }
    if let Some(a) = args.get(1) {
        match ShapeKind::parse(a) {
            Some(k) => sel.shape = k,
            None => {
                return fail(
                    game,
                    &format!("Shape {a} not found. Available shapes: {}", ShapeKind::names()),
                );
            }
        }
    }
    if args.len() >= 3 {
        match (
            input::coordinate(args[2]),
            args.get(3).and_then(|a| input::coordinate(a)),
        ) {
            (Some(x), Some(z)) => {
                sel.center_x = x;
                sel.center_z = z;
            }
            _ => return fail(game, "Invalid centre: give both x and z."),
        }
    }
    if let Some(a) = args.get(4) {
        let Some(rx) = input::radius(a, sel.radius_x) else {
            return fail(game, "Invalid radius (0 to 30000000).");
        };
        let rz = match args.get(5) {
            Some(b) => match input::radius(b, sel.radius_z) {
                Some(v) => v,
                None => return fail(game, "Invalid z radius."),
            },
            None => rx,
        };
        sel.radius_x = rx;
        sel.radius_z = rz;
    }
    st.selection = sel.clone();
    let world = sel.world.clone();
    if st.tasks.contains_key(&world) {
        return fail(game, &format!("Task already started for {world}!"));
    }
    let fresh = SavedTask {
        selection: sel.clone(),
        owner: Some(sender.name.clone()),
        owner_uuid: sender.uuid,
        cancelled: false,
        chunks: 0,
        time_ms: 0,
    };
    if read_task(st, &world).is_some_and(|t| !t.cancelled) {
        st.pending
            .insert(sender.name.clone(), (Pending::Start(fresh), Instant::now()));
        say(
            game,
            "A task was already started for this world. To continue running it, type '/chunky continue'. To start a new task, type '/chunky confirm'.",
        );
        return;
    }
    announce_start(game, &sel);
    start_task(game, st, fresh);
}

fn announce_start(game: &mut Game<'_>, sel: &Selection) {
    say(
        game,
        &format!(
            "Task started in {} for the {} region centered at {}, {} with radius {}.",
            sel.world,
            sel.shape.name(),
            input::fmt_number(sel.center_x),
            input::fmt_number(sel.center_z),
            if sel.shape.two_radii() {
                format!(
                    "{}, {}",
                    input::fmt_number(sel.radius_x),
                    input::fmt_number(sel.radius_z)
                )
            } else {
                input::fmt_number(sel.radius_x)
            }
        ),
    );
}

fn start_task(game: &mut Game<'_>, st: &mut State, saved: SavedTask) {
    let sel = saved.selection.clone();
    let world = sel.world.clone();
    let iter = ChunkIter::new(&sel, saved.chunks);
    let now = Instant::now();
    // A level that isn't the one in place may not be open: a ticket opens it at the next
    // server tick, and its tick then fills the window.
    let center = (sel.center_chunk_x(), sel.center_chunk_z());
    let keepalive = game
        .native_force_chunk_in(TICKET, &world, [center.0, center.1], true)
        .then_some(center);
    log::info!(
        "Chunky: task for {world}: {} {} around {} {}, radius {}, {} chunks, from {}",
        sel.shape.name(),
        sel.pattern.name(),
        sel.center_x,
        sel.center_z,
        sel.radius_x,
        iter.total,
        saved.chunks
    );
    st.tasks.insert(
        world,
        Task {
            shape: Shape::new(&sel),
            sel,
            done: iter.index,
            iter,
            window: VecDeque::new(),
            forced: 0,
            owner: saved.owner,
            owner_uuid: saved.owner_uuid,
            before_ms: saved.time_ms,
            started: now,
            rate: Rate::default(),
            current: center,
            last_update: now,
            last_progress: now,
            last_save: now,
            keepalive,
        },
    );
    // Written now: a crash before the first autosave still leaves the task to continue.
    if let Some(t) = st.tasks.get(&saved.selection.world) {
        write_task(st.world.as_ref(), &t.saved(false));
    }
}

/// Let go of everything a task holds.
fn stop(game: &mut Game<'_>, task: &Task) {
    let world = &task.sel.world;
    for slot in task.window.iter().filter(|s| s.forced && !s.done) {
        game.native_force_chunk_in(TICKET, world, [slot.key.0, slot.key.1], false);
    }
    if let Some(k) = task.keepalive {
        game.native_force_chunk_in(TICKET, world, [k.0, k.1], false);
    }
}

fn cancel(game: &mut Game<'_>, st: &mut State, world: Option<String>) {
    let worlds: Vec<String> = match &world {
        Some(w) => vec![w.clone()],
        None => {
            say(game, "Cancelling all tasks.");
            WORLDS.iter().map(|w| (*w).to_owned()).collect()
        }
    };
    let mut any = false;
    for w in worlds {
        if let Some(task) = st.tasks.remove(&w) {
            stop(game, &task);
            write_task(st.world.as_ref(), &task.saved(true));
            say(game, &format!("Task cancelled for {w}."));
            any = true;
        } else if let Some(mut saved) = read_task(st, &w).filter(|t| !t.cancelled) {
            saved.cancelled = true;
            write_task(st.world.as_ref(), &saved);
            say(game, &format!("Task cancelled for {w}."));
            any = true;
        }
    }
    if !any {
        fail(game, "No tasks to cancel.");
    }
}

fn now_ms(t: &Task) -> u64 {
    t.time_ms()
}

// ---- Ticks ---------------------------------------------------------------------------------

/// Whether the chunk is done: loaded, lit and post-processed (FULL), as `/pregen` waits for.
fn ready(game: &Game<'_>, (x, z): (i32, i32)) -> bool {
    game.streaming.is_lit(x, z)
        && game
            .world
            .chunks
            .get(&(x, z))
            .is_some_and(|c| c.postprocess.is_empty())
}

/// After a level's tick, with that level in place: count what finished, hold the next
/// chunks, report.
pub(in crate::game) fn level_tick(game: &mut Game<'_>, dimension: &str) {
    let Some(mut guard) = state() else {
        return;
    };
    let Some(st) = guard.as_mut() else {
        return;
    };
    if game.dim.id() != dimension {
        return;
    }
    let (window, silent, interval) = (
        st.config.window,
        st.config.silent,
        Duration::from_secs(st.config.update_interval),
    );
    let Some(task) = st.tasks.get_mut(dimension) else {
        return;
    };
    let now = Instant::now();

    // The chunks held that are done now.
    let mut finished = 0u64;
    for slot in task.window.iter_mut().filter(|s| s.forced && !s.done) {
        if ready(game, slot.key) {
            slot.done = true;
            game.native_force_chunk_in(TICKET, dimension, [slot.key.0, slot.key.1], false);
            finished += 1;
        }
    }
    if finished > 0 {
        task.forced -= finished as usize;
        let ms = task.time_ms();
        task.rate.add(ms, finished);
        task.last_progress = now;
    }
    while task.window.front().is_some_and(|s| s.done) {
        task.window.pop_front();
        task.done += 1;
    }

    // The next chunks, until the window is full. Chunks outside the shape and chunks
    // already done count at once; a long run of those is bounded by the queue's size.
    let max_queue = window * 4;
    while task.forced < window && task.window.len() < max_queue {
        let Some(key) = task.iter.next() else {
            break;
        };
        task.current = key;
        let skip = !task.shape.contains_chunk(key.0, key.1) || ready(game, key);
        let forced = !skip && game.native_force_chunk_in(TICKET, dimension, [key.0, key.1], true);
        if forced {
            task.forced += 1;
        }
        task.window.push_back(Slot {
            key,
            forced,
            done: !forced,
        });
    }
    while task.window.front().is_some_and(|s| s.done) {
        task.window.pop_front();
        task.done += 1;
    }
    if let Some(k) = task.keepalive.take() {
        game.native_force_chunk_in(TICKET, dimension, [k.0, k.1], false);
    }

    // Finished.
    if task.iter.left() == 0 && task.window.is_empty() {
        let task = st.tasks.remove(dimension).expect("the task");
        let line = progress::done_line(dimension, task.done, task.iter.total, task.time_ms());
        report(game, task.owner_uuid, &line);
        write_task(st.world.as_ref(), &task.saved(true));
        return;
    }

    // Stuck: say what the window waits on, and queue light again for chunks that lost
    // their job (as `/pregen` does).
    if task.forced > 0 && now.duration_since(task.last_progress) >= STALL {
        task.last_progress = now;
        let waiting: Vec<(i32, i32)> = task
            .window
            .iter()
            .filter(|s| s.forced && !s.done)
            .map(|s| s.key)
            .collect();
        let [missing, lighting, unlit] = game.streaming.unstick_light(&mut game.world, &waiting);
        log::warn!(
            "Chunky: {dimension} waits on {} chunks: {missing} not loaded, {lighting} being lit, \
             {unlit} unlit (light queued again); {:?}",
            waiting.len(),
            game.streaming.stages()
        );
    }

    if !silent && now.duration_since(task.last_update) >= interval {
        task.last_update = now;
        let rate = task.rate.rate(task.time_ms());
        let line = progress::update_line(dimension, task.done, task.iter.total, rate, task.current);
        let owner = task.owner_uuid;
        report(game, owner, &line);
    }
    let Some(task) = st.tasks.get_mut(dimension) else {
        return;
    };
    if now.duration_since(task.last_save) >= AUTOSAVE {
        task.last_save = now;
        write_task(st.world.as_ref(), &task.saved(false));
    }
}

/// Each server tick: once the world is open, continue the saved tasks when the settings
/// ask for it; forget a confirm nobody gave.
pub(in crate::game) fn server_tick(game: &mut Game<'_>) {
    let Some(mut guard) = state() else {
        return;
    };
    let st = init(&mut *guard);
    if st.booted {
        return;
    }
    let Some(dir) = game.native_world_dir() else {
        return;
    };
    st.world = Some(dir);
    st.booted = true;
    st.pending.retain(|_, (_, at)| at.elapsed() <= CONFIRM);
    if !st.config.continue_on_restart {
        return;
    }
    for w in WORLDS {
        if let Some(saved) = read_task(st, w).filter(|t| !t.cancelled) {
            log::info!("Chunky: continuing the task for {w} (continue-on-restart)");
            start_task(game, st, saved);
        }
    }
}

/// The world saved, or the server stops: the running tasks' files are written, so
/// `/chunky continue` goes on from here.
pub(in crate::game) fn flush() {
    let Some(guard) = state() else {
        return;
    };
    let Some(st) = guard.as_ref() else {
        return;
    };
    for task in st.tasks.values() {
        write_task(st.world.as_ref(), &task.saved(false));
    }
}
