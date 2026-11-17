//! `config/chunky/config.properties`: written with the defaults when missing,
//! read again by `/chunky reload`; `quiet` and `silent` write it.

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Continue the tasks saved when the server stopped, once it starts.
    pub continue_on_restart: bool,
    /// No progress lines (the finished line is still sent).
    pub silent: bool,
    /// Seconds between progress lines.
    pub update_interval: u64,
    /// How many chunks a task holds loaded at once while they generate.
    pub window: usize,
    /// The operator level `/chunky` needs (0 to 4); read at start-up only.
    pub permission: u8,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            continue_on_restart: false,
            silent: false,
            update_interval: 5,
            window: 256,
            permission: 2,
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Config {
        let mut c = Config::default();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim();
            match k.trim() {
                "continue-on-restart" => c.continue_on_restart = v == "true",
                "silent" => c.silent = v == "true",
                "update-interval" => c.update_interval = v.parse().unwrap_or(c.update_interval),
                "window" => c.window = v.parse::<usize>().unwrap_or(c.window).clamp(1, 4096),
                "permission" => c.permission = v.parse::<u8>().unwrap_or(c.permission).min(4),
                _ => {}
            }
        }
        c
    }

    pub fn write(&self) -> String {
        format!(
            "# Chunky for Horizon\n\
             # Continue the tasks that were running when the server stopped (true/false).\n\
             continue-on-restart={}\n\
             # No progress lines while a task runs (/chunky silent).\n\
             silent={}\n\
             # Seconds between progress lines (/chunky quiet <seconds>).\n\
             update-interval={}\n\
             # Chunks a task keeps loading at once (1-4096). More is faster and takes more memory.\n\
             window={}\n\
             # Operator level needed for /chunky (0-4); takes effect after a restart.\n\
             permission={}\n",
            self.continue_on_restart, self.silent, self.update_interval, self.window, self.permission
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let c = Config {
            continue_on_restart: true,
            silent: true,
            update_interval: 30,
            window: 64,
            permission: 4,
        };
        assert_eq!(Config::parse(&c.write()), c);
        assert_eq!(Config::parse("window=999999"). window, 4096);
    }
}
