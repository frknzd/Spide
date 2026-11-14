//! Real PTY I/O, independent of design snapshots. Binary bytes stay binary up to xterm.
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

const CAP: usize = 4 * 1024 * 1024;
const REPLAY: usize = 128 * 1024;
const BATCH: usize = 32 * 1024;

#[derive(Default)]
struct Output {
    bytes: VecDeque<u8>,
    start: u64,
    ack: u64,
    exited: bool,
    stopped: bool,
}
#[derive(Default)]
struct Stream { state: Mutex<Output>, wake: Condvar }

#[derive(Serialize, Debug)]
pub struct Frame {
    pub generation: u64,
    pub start: u64,
    pub next: u64,
    pub bytes: Vec<u8>,
    pub exited: bool,
    pub reset: bool,
}

pub struct Terminal {
    pub generation: u64,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    stream: Arc<Stream>,
    pid: Option<u32>,
}

impl Terminal {
    pub fn spawn(cmd: CommandBuilder, generation: u64) -> Result<Arc<Self>, String> {
        let pair = native_pty_system().openpty(PtySize { rows: 40, cols: 90, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())?;
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let pid = child.process_id();
        let child = Arc::new(Mutex::new(child));
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let stream = Arc::new(Stream::default());
        let out = stream.clone();
        let reaper = child.clone();
        let exit = stream.clone();
        std::thread::spawn(move || {
            loop {
                if exit.state.lock().unwrap().stopped { break; }
                if matches!(reaper.lock().unwrap().try_wait(), Ok(Some(_))) {
                    exit.state.lock().unwrap().exited = true;
                    exit.wake.notify_all();
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 { break; }
                let mut s = out.state.lock().unwrap();
                while s.bytes.len() + n > CAP && !s.stopped {
                    s = out.wake.wait(s).unwrap();
                }
                // Keep draining on shutdown: macOS can leave a writer stuck in tty output even
                // after SIGKILL until the PTY is read. Shutdown output is deliberately discarded.
                if s.stopped { continue; }
                s.bytes.extend(&buf[..n]);
                out.wake.notify_all();
            }
            let mut s = out.state.lock().unwrap();
            s.exited = true;
            out.wake.notify_all();
        });
        Ok(Arc::new(Self { generation, child, master: Mutex::new(pair.master), writer: Mutex::new(writer), stream, pid }))
    }
    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        if data.len() > 1024 * 1024 { return Err("terminal input exceeds 1 MiB".into()); }
        let mut w = self.writer.lock().unwrap();
        w.write_all(data).and_then(|_| w.flush()).map_err(|e| e.to_string())
    }
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.master.lock().unwrap().resize(PtySize { cols: cols.clamp(10, 500), rows: rows.clamp(3, 300), pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())
    }
    /// Read after the last *rendered* byte. That cursor acknowledges previous data: slow renderers
    /// backpressure the reader rather than dropping bytes or cutting UTF-8/escape sequences.
    pub fn read(&self, after: u64) -> Result<Frame, String> {
        let mut s = self.stream.state.lock().unwrap();
        let tail = s.start + s.bytes.len() as u64;
        if after > tail { return Err("invalid terminal cursor".into()); }
        s.ack = s.ack.max(after);
        let trim_to = s.ack.saturating_sub(REPLAY as u64);
        while s.start < trim_to && !s.bytes.is_empty() { s.bytes.pop_front(); s.start += 1; }
        self.stream.wake.notify_all();
        if after >= s.start + s.bytes.len() as u64 && !s.exited && !s.stopped {
            s = self.stream.wake.wait_timeout(s, Duration::from_millis(50)).unwrap().0;
        }
        let start = after.max(s.start);
        let bytes: Vec<u8> = s.bytes.iter().skip((start - s.start) as usize).take(BATCH).copied().collect();
        Ok(Frame { generation: self.generation, start, next: start + bytes.len() as u64, reset: after < s.start, bytes, exited: s.exited })
    }
    pub fn stop(&self) {
        {
            let mut s = self.stream.state.lock().unwrap();
            if s.stopped { return; }
            s.stopped = true;
            self.stream.wake.notify_all();
        }
        // Tool shells may create their own session/group. Freeze and collect the *owned*
        // process tree before termination, rather than killing other pi instances by name.
        let running = !matches!(self.child.lock().unwrap().try_wait(), Ok(Some(_)));
        let owned = if running { self.pid.map(owned_processes).unwrap_or_default() } else { vec![] };
        signal_processes(&owned, libc::SIGTERM);
        signal_processes(&owned, libc::SIGCONT);
        let mut c = self.child.lock().unwrap();
        let mut reaped = false;
        for _ in 0..20 {
            if matches!(c.try_wait(), Ok(Some(_))) { reaped = true; break; }
            std::thread::sleep(Duration::from_millis(25));
        }
        signal_processes(&owned, libc::SIGKILL);
        if !reaped { let _ = c.wait(); }
    }
}
impl Drop for Terminal { fn drop(&mut self) { self.stop(); } }

fn signal_processes(pids: &[u32], signal: i32) {
    for pid in pids.iter().rev() { unsafe { libc::kill(*pid as i32, signal); } }
}
fn owned_processes(root: u32) -> Vec<u32> {
    let mut owned = vec![root];
    unsafe { libc::kill(root as i32, libc::SIGSTOP); }
    // Freeze each newly discovered descendant before the next pass, including detached tool
    // shells. No process-name matching or signals to the application's own process group.
    for _ in 0..4 {
        let Ok(ps) = std::process::Command::new("/bin/ps").args(["-A", "-o", "pid=,ppid="]).output() else { break };
        let pairs: Vec<(u32,u32)> = String::from_utf8_lossy(&ps.stdout).lines().filter_map(|l| {
            let mut cols = l.split_whitespace(); Some((cols.next()?.parse().ok()?, cols.next()?.parse().ok()?))
        }).collect();
        let before = owned.len();
        loop {
            let next: Vec<u32> = pairs.iter().filter(|(pid,parent)| owned.contains(parent) && !owned.contains(pid)).map(|(pid,_)| *pid).collect();
            if next.is_empty() { break; }
            signal_processes(&next, libc::SIGSTOP);
            owned.extend(next);
        }
        if before == owned.len() { break; }
    }
    owned
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bytes_resize_and_exit() {
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", r#"printf '\033[31m'; printf '\342'; sleep .05; printf '\202\254'; read x; printf 'done:%s' "$x""#]);
        let t = Terminal::spawn(cmd, 9).unwrap();
        t.resize(100, 35).unwrap();
        t.write(b"hello\n").unwrap();
        let mut after = 0;
        let mut bytes = vec![];
        for _ in 0..100 {
            let f = t.read(after).unwrap();
            assert_eq!(f.generation, 9);
            assert_eq!(f.start, after);
            after = f.next;
            bytes.extend(f.bytes);
            if f.exited { break; }
        }
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.contains("\u{1b}[31m€"), "{s:?}");
        assert!(s.contains("done:hello"));
        assert!(t.read(after + 100).is_err());
        t.stop();
    }
    #[test]
    fn stops_owned_detached_descendants() {
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "python3 -c 'import os,time; os.setsid(); time.sleep(60)' & printf 'owned:%s\\n' $!; wait"]);
        let t = Terminal::spawn(cmd, 2).unwrap();
        let mut text = String::new();
        let mut cursor = 0;
        for _ in 0..40 {
            let f = t.read(cursor).unwrap(); cursor = f.next;
            text.push_str(&String::from_utf8_lossy(&f.bytes));
            if text.contains('\n') { break; }
        }
        let pid: u32 = text.trim().strip_prefix("owned:").unwrap().parse().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        t.stop();
        std::thread::sleep(Duration::from_millis(100));
        let ps = std::process::Command::new("/bin/ps").args(["-p", &pid.to_string(), "-o", "stat="]).output().unwrap();
        let stat = String::from_utf8_lossy(&ps.stdout);
        assert!(stat.trim().is_empty() || stat.contains('Z'), "owned descendant survived: {stat}");
    }
    #[test]
    fn bounded_backpressure_and_replay() {
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "yes abcdefghijklmnopqrstuvwxyz"]);
        let t = Terminal::spawn(cmd, 1).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let mut after = 0;
        for _ in 0..200 {
            let f = t.read(after).unwrap();
            assert_eq!(f.start, after);
            after = f.next;
            assert!(t.stream.state.lock().unwrap().bytes.len() <= CAP);
        }
        assert!(after > REPLAY as u64);
        assert!(t.read(0).unwrap().reset);
        t.stop();
        t.stop();
    }
}
