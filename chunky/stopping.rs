// The server is stopping: the running tasks are saved, `/chunky continue` resumes them.
fn chunky_stopping() {
    crate::game::chunky_glue::flush();
}
