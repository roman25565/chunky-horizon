//! Progress numbers as Chunky reports them: the rate over the last 30
//! seconds in one-second bins, the time left, and the lines.
use std::collections::VecDeque;

/// How far back the rate looks, in milliseconds.
const SAMPLE_MS: u64 = 30_000;
const BIN_MS: u64 = 1_000;

#[derive(Default, Debug)]
pub struct Rate {
    bins: VecDeque<(u64, u64)>,
}

impl Rate {
    /// `n` chunks generated at `now_ms`.
    pub fn add(&mut self, now_ms: u64, n: u64) {
        match self.bins.back_mut() {
            Some((t, c)) if now_ms.saturating_sub(*t) < BIN_MS => *c += n,
            _ => self.bins.push_back((now_ms, n)),
        }
        self.prune(now_ms);
    }

    fn prune(&mut self, now_ms: u64) {
        while self
            .bins
            .front()
            .is_some_and(|(t, _)| now_ms.saturating_sub(*t) > SAMPLE_MS)
        {
            self.bins.pop_front();
        }
    }

    /// Chunks per second over the bins kept (0 before a tenth of a second).
    pub fn rate(&mut self, now_ms: u64) -> f64 {
        self.prune(now_ms);
        let Some(&(oldest, _)) = self.bins.front() else {
            return 0.0;
        };
        let secs = now_ms.saturating_sub(oldest) as f64 / 1000.0;
        if secs < 0.1 {
            return 0.0;
        }
        self.bins.iter().map(|(_, c)| *c).sum::<u64>() as f64 / secs
    }
}

/// `H:MM:SS` as Chunky prints it (`%01d:%02d:%02d`).
pub fn hms(seconds: u64) -> String {
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

pub fn percent(done: u64, total: u64) -> f64 {
    if total == 0 {
        100.0
    } else {
        100.0 * done as f64 / total as f64
    }
}

/// Chunky's `task_update` line.
pub fn update_line(
    world: &str,
    done: u64,
    total: u64,
    rate: f64,
    current: (i32, i32),
) -> String {
    let left = total.saturating_sub(done);
    let eta = if rate > 0.0 {
        hms((left as f64 / rate) as u64)
    } else {
        "?:??:??".into()
    };
    format!(
        "Task running for {world}. Processed: {done} chunks ({:.2}%), ETA: {eta}, Rate: {rate:.1} cps, Current: {}, {}",
        percent(done, total),
        current.0,
        current.1
    )
}

/// Chunky's `task_done` line.
pub fn done_line(world: &str, done: u64, total: u64, time_ms: u64) -> String {
    format!(
        "Task finished for {world}. Processed: {done} chunks ({:.2}%), Total time: {}",
        percent(done, total),
        hms(time_ms / 1000)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_and_lines() {
        let mut r = Rate::default();
        for s in 0..10 {
            r.add(s * 1000, 50);
        }
        let v = r.rate(10_000);
        assert!((v - 50.0).abs() < 1e-9, "{v}");
        assert_eq!(r.rate(100_000), 0.0);
        assert_eq!(hms(3725), "1:02:05");
        assert_eq!(
            update_line("minecraft:overworld", 50, 200, 10.0, (3, -4)),
            "Task running for minecraft:overworld. Processed: 50 chunks (25.00%), ETA: 0:00:15, Rate: 10.0 cps, Current: 3, -4"
        );
    }
}
