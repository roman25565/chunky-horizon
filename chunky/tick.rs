// Each server tick: continue saved tasks once the world is open (continue-on-restart).
fn chunky_tick(game: &mut crate::game::Game<'_>) {
    crate::game::chunky_glue::server_tick(game);
}
