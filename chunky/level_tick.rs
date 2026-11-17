// After a level's tick, with it in place: the task of that level counts and holds chunks.
fn chunky_level_tick(game: &mut crate::game::Game<'_>, dimension: &str) {
    crate::game::chunky_glue::level_tick(game, dimension);
}
