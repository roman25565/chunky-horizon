# Chunky for Horizon

A server-side mod for [Horizon](https://github.com/horizon-rust/horizon) (Minecraft 26.3 in Rust): a port of
**[Chunky](https://github.com/pop4959/Chunky)**, the chunk pregenerator. You choose an area (world, center,
radius, shape, pattern), and the mod generates, lights and saves every chunk in it while the server keeps running.
Players install nothing.

> **Credits and license.** A derivative work of Chunky (by pop4959 and contributors, GPL-3.0). The shape formulas,
> patterns, commands and message texts come from Chunky; the code is written from scratch in Rust.

## Commands

All commands: `/chunky ...` in game (operator level 2, configurable) or `chunky ...` in the server console
(`horizon-server --dir <folder> cmd chunky start`).

| Command | What it does |
|---|---|
| `chunky start [world] [shape] [x z] [radius] [radiusZ]` | start a task from the current selection (arguments change it) |
| `chunky pause [world]` | pause: progress is written to a file, chunks are released |
| `chunky continue [world]` | continue a saved task (also after a server restart) |
| `chunky cancel [world]` + `chunky confirm` | cancel for good (without a world: all tasks) |
| `chunky confirm` | confirm a cancel or a new start over a saved task |
| `chunky progress` | progress of all tasks |
| `chunky center [x z]` | center (without arguments: where the player stands) |
| `chunky spawn` | center at the world spawn |
| `chunky worldborder [world]` | center and radius from the world border |
| `chunky radius <r> [rz]` | radius in blocks; `20c` chunks (x16), `2r` regions (x512), `1.5k` thousands; `+100`/`-100` relative to the current one |
| `chunky corners <x1> <z1> <x2> <z2>` | center and radii from two corners (shape square/rectangle) |
| `chunky shape <shape>` | `square`, `circle`, `rectangle`, `ellipse` (`oval`), `diamond`, `pentagon`, `hexagon`, `star`, `triangle` |
| `chunky pattern <pattern>` | `region` (default), `concentric`, `loop`, `spiral` |
| `chunky world [world]` | `overworld`, `the_nether` (`nether`), `the_end` (`end`); without an argument the player's world |
| `chunky selection` | show the current selection |
| `chunky quiet <seconds>` | how often to print progress |
| `chunky silent` | turn progress lines on/off |
| `chunky reload` | reload the config |
| `chunky help` | list of commands |

Progress goes to the server log and to whoever started the task, in Chunky's format:
`Task running for minecraft:overworld. Processed: 1234 chunks (45.67%), ETA: 0:01:23, Rate: 98.7 cps, Current: 12, -3`.
At the end: `Task finished for ... Total time: ...`.

The world in a command is written without `minecraft:` (`nether`, `the_end`), because that is what the game's command tree accepts.

## How it works

- A task keeps a "window" of `window` chunks (default 256) with mod chunk tickets (`native_force_chunk_in`,
  the same as `/forceload`). Level streaming generates, decorates and lights them, and the server feeds
  the workers between ticks (`pump_chunks`), so the server doesn't freeze.
- After a level tick the mod releases chunks that are already lit and fully processed (FULL, as Horizon's `/pregen` waits for);
  they are unloaded and saved to region files, and the next ones are taken. Only the window is in memory, whatever the radius.
- Chunks outside the shape and chunks already generated (loaded from disk with lighting) are counted immediately.
- Tasks for different worlds run in parallel (one per world). A level with nobody in it is opened with a ticket.

## Files

```
chunky/                        the mod: horizon-mixin.toml, glue.rs (game), core/ (logic without the game), hooks *.rs
  core/                        shapes, patterns, selection, task files, input parsing: cargo test
  tools/pack-hnmod.py          build a .hnmod
config/chunky/config.properties   settings (created on first start)
world/chunky/tasks/minecraft/<world>.properties   saved tasks (like TaskLoader in Chunky)
```

`config.properties` settings:

- `continue-on-restart=false` continue tasks automatically after a server restart;
- `silent=false`, `update-interval=5` (seconds between progress lines; Chunky's default is 1);
- `window=256` how many chunks are generated at once (more is faster but uses more memory);
- `permission=2` operator level for `/chunky` (after a restart).

Building: put the `chunky` folder next to your other Horizon mods and build the engine with them.

## Missing or different from Chunky

- **`trim` is not implemented.** Deleting saved chunks from region files while the server runs is unsafe
  (the engine keeps them open and may write a chunk back), and Horizon gives mods no safe API for it.
  The command replies that it isn't available.
- **World border**: `worldborder` reads the border (`native_world_border`). There are no integrations with border plugins
  (WorldBorder, Chunky Border). Horizon currently keeps one border for all worlds.
- **Patterns**: `ring` (Chunky's new default), `strip`, `csv` and `world` are not ported; the default here is `region`.
  Inside a region chunks go row by row rather than along a Hilbert curve (the set of chunks is the same).
  `rectangle`/`ellipse` always use the `loop` pattern (Chunky uses `strip` there).
- **Progress in the file** is stored as the number of chunks done *in a row* in pattern order, so after a
  pause a few already finished chunks are visited again (they are simply loaded from disk).
- `limit` (rate limit), `debug`, message languages and `forceLoadExistingChunks` are not ported.
  Texts are in English, as in Chunky.
- Chunks in the window tick like `/forceload`ed chunks while they generate (harmless for mobs, but a window of 4096
  can lag a weak PC).
- The mod is pinned to Horizon commit `fdc87ae` (it reads the game's `streaming` and `world.chunks` directly to know
  that a chunk is ready). After updating Horizon, check and update `horizon_revision`.

## License and credits

**GNU GPL v3** (see [LICENSE](LICENSE)), because this is a derivative work of [Chunky](https://github.com/pop4959/Chunky) (© pop4959 and contributors, GPL-3.0). The shape formulas, patterns, commands and message texts come from Chunky; the code is written from scratch in Rust. A mod for [Horizon](https://github.com/horizon-rust/horizon) (MIT, Ludovic Marie and the Horizon contributors).

Unofficial project, not affiliated with Mojang Studios or Microsoft. Minecraft is a trademark of Mojang Synergies AB.
