// A player's command on the server: /chunky, when it is one.
fn chunky_command(
    game: &mut crate::game::Game<'_>,
    player: &crate::game::native_server::NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    if !*ran {
        crate::game::chunky_glue::perform(game, player, command, ran);
    }
}
