// Chunky's /chunky usages in the command tree.
fn chunky_register(commands: &mut Vec<crate::game::native_server::NativeCommand>) {
    crate::game::chunky_glue::commands(commands);
}
