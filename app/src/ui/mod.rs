//! Native egui user interface.
pub mod app;
pub mod brief;
pub mod canvas;
pub mod chrome;
pub mod decisions;
pub mod export;
pub mod export_jobs;
pub mod hover_card;
pub mod overlays;
pub mod paint;
pub mod project;
pub mod proposal;
pub mod state;

pub mod terminal;
pub mod theme;
pub mod timeline;
pub mod trace;
pub mod viewport;
pub mod views;
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
