//! Native egui user interface.
pub mod actions;
pub mod activity;
pub mod app;
pub mod branches;
pub mod backup;
pub mod brief;
pub mod canvas;
pub mod clipboard;
pub mod chrome;
pub mod decisions;
pub mod draft_recovery;
pub mod export;
pub mod export_jobs;
pub mod help;
pub mod hover_card;
pub mod integrations;
pub mod leave;
pub mod navigate;
pub mod overlays;
pub mod opening;
pub mod paint;
pub mod project;
pub mod properties;
pub mod proposal;
pub mod quick_export;
pub mod search;
pub mod selection;
pub mod settings;
pub mod state;

pub mod terminal;
pub mod theme;
pub mod timeline;
pub mod trace;
pub mod viewport;
pub mod views;
pub mod welcome;
pub mod workspace_memory;
pub mod widgets;
#[cfg(test)]
mod tests;

fn local(ms: u64) -> libc::tm {
    let secs = (ms / 1000) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&secs, &mut tm);
    }
    tm
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// Local date and time (`14/11/2025, 10:47:00`), like `toLocaleString()`.
pub fn format_time(ms: u64) -> String {
    let t = local(ms);
    format!("{:02}/{:02}/{}, {:02}:{:02}:{:02}", t.tm_mday, t.tm_mon + 1, t.tm_year + 1900, t.tm_hour, t.tm_min, t.tm_sec)
}

/// Today as `14 November 2025` (export headers).
pub fn format_date_today() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let t = local(now);
    format!("{} {} {}", t.tm_mday, MONTHS[t.tm_mon.clamp(0, 11) as usize], t.tm_year + 1900)
}
