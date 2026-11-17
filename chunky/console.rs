// A command typed on the server's console: chunky, when it is one.
fn chunky_console(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    if !*ran {
        crate::game::chunky_glue::console(game, command, ran);
    }
}
