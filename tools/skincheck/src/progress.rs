//! A one-line progress bar for runs that take minutes.
//!
//! Rewrites a single stderr line as the work goes, so a run over tens of
//! thousands of skins shows movement instead of going quiet. Every line ends in
//! `\r`, so a redirected log reads as one long line that `grep -o` can still
//! pull counts out of.

use std::io::Write;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Longest the bar is, in characters, so it fits an 80-column terminal.
const WIDTH: usize = 28;
/// Redraw at most this often; workers tick far faster than a person can read.
const REDRAW: Duration = Duration::from_millis(100);

pub struct Bar {
    label: &'static str,
    total: usize,
    start: Instant,
    last: Mutex<Instant>,
}

impl Bar {
    pub fn new(label: &'static str, total: usize) -> Self {
        let bar = Bar { label, total, start: Instant::now(), last: Mutex::new(Instant::now() - REDRAW) };
        bar.draw(0);
        bar
    }

    /// Record that `done` units have finished. Redraws at most every 100ms.
    pub fn tick(&self, done: usize) {
        let mut last = self.last.lock().unwrap();
        if last.elapsed() < REDRAW && done < self.total {
            return;
        }
        *last = Instant::now();
        drop(last);
        self.draw(done);
    }

    /// Finish and move off the line, so the shell prompt does not land on it.
    pub fn finish(&self) {
        self.draw(self.total);
        eprintln!();
    }

    fn draw(&self, done: usize) {
        let frac = if self.total == 0 { 1.0 } else { done as f64 / self.total as f64 };
        let filled = ((frac * WIDTH as f64).round() as usize).min(WIDTH);
        let secs = self.start.elapsed().as_secs_f64();
        let rate = if secs > 0.0 { done as f64 / secs } else { 0.0 };
        let eta = if rate > 0.0 { (self.total - done) as f64 / rate } else { 0.0 };
        eprint!(
            "\r{} [{}{}] {:5.1}%  {done:>7}/{}  {:6.0}/s  eta {:>5}:{:02}   ",
            self.label,
            "#".repeat(filled),
            "-".repeat(WIDTH - filled),
            frac * 100.0,
            self.total,
            rate,
            (eta / 60.0) as u64,
            (eta % 60.0) as u64,
        );
        let _ = std::io::stderr().flush();
    }
}