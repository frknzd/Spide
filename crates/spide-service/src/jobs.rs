//! Long-running jobs (checks, analysis) with ids, status, cancellation and timeouts.
//! Job state is persisted in `.spide/jobs/<id>.json`; jobs left `running` by a dead process are
//! marked `interrupted` on startup instead of being reported as finished.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const OUTPUT_CAP: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub state: JobState,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    /// Exit status, kept separate from findings: a crashed linter is not a clean run.
    pub exit_code: Option<i32>,
    pub output: String,
    pub op: Option<String>,
}

#[derive(Clone)]
pub struct Jobs {
    dir: PathBuf,
    inner: Arc<Mutex<BTreeMap<String, (Job, Option<Arc<Mutex<Child>>>)>>>,
    counter: Arc<Mutex<u64>>,
    flags: Arc<Mutex<BTreeMap<String, Arc<std::sync::atomic::AtomicBool>>>>,
}

fn tail(mut s: String) -> String {
    if s.len() > OUTPUT_CAP {
        let mut cut = s.len() - OUTPUT_CAP;
        while !s.is_char_boundary(cut) {
            cut += 1;
        }
        s = format!("[… {cut} bytes elided]\n{}", &s[cut..]);
    }
    crate::trace::redact(&s)
}

impl Jobs {
    pub fn new(root: &Path) -> Jobs {
        Jobs { dir: root.join(".spide").join("jobs"), inner: Default::default(), counter: Default::default(), flags: Default::default() }
    }

    fn persist(&self, j: &Job) {
        let _ = std::fs::create_dir_all(&self.dir);
        let p = self.dir.join(format!("{}.json", j.id));
        let _ = crate::workspace::write_atomic(&p, &serde_json::to_string_pretty(j).unwrap());
    }

    /// Mark persisted running jobs of an earlier process as interrupted.
    pub fn recover(&self) -> Vec<String> {
        let mut out = vec![];
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return out };
        for e in rd.flatten() {
            let Ok(t) = std::fs::read_to_string(e.path()) else { continue };
            let Ok(mut j) = serde_json::from_str::<Job>(&t) else { continue };
            if j.state == JobState::Running {
                j.state = JobState::Interrupted;
                j.finished_at = Some(spide_core::store::now_ms());
                self.persist(&j);
                out.push(j.id.clone());
            }
            self.inner.lock().unwrap().entry(j.id.clone()).or_insert((j, None));
        }
        out
    }

    fn next_id(&self) -> String {
        let mut c = self.counter.lock().unwrap();
        *c += 1;
        format!("J{}-{}", spide_core::store::now_ms(), *c)
    }

    /// Run `sh -c command` in `cwd` with a timeout. `done` gets the finished job (on the job thread).
    pub fn spawn_command(
        &self,
        kind: &str,
        label: &str,
        command: &str,
        cwd: &Path,
        timeout: Duration,
        op: Option<String>,
        done: impl FnOnce(&Job) + Send + 'static,
    ) -> Result<String, String> {
        let id = self.next_id();
        let child = Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not start {label}: {e}"))?;
        let job = Job { id: id.clone(), kind: kind.into(), label: label.into(), state: JobState::Running, started_at: spide_core::store::now_ms(), finished_at: None, exit_code: None, output: String::new(), op };
        self.persist(&job);
        let child = Arc::new(Mutex::new(child));
        self.inner.lock().unwrap().insert(id.clone(), (job, Some(child.clone())));
        let me = self.clone();
        let jid = id.clone();
        std::thread::spawn(move || {
            let (out, err) = {
                let mut c = child.lock().unwrap();
                (c.stdout.take(), c.stderr.take())
            };
            let read = |s: Option<Box<dyn std::io::Read + Send>>| {
                std::thread::spawn(move || {
                    let mut b = Vec::new();
                    if let Some(mut s) = s {
                        let _ = s.read_to_end(&mut b);
                    }
                    String::from_utf8_lossy(&b).into_owned()
                })
            };
            let ho = read(out.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>));
            let he = read(err.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>));
            let start = Instant::now();
            let mut timed_out = false;
            let status = loop {
                if let Ok(Some(s)) = child.lock().unwrap().try_wait() {
                    break Some(s);
                }
                if start.elapsed() > timeout {
                    let _ = child.lock().unwrap().kill();
                    timed_out = true;
                    break child.lock().unwrap().wait().ok();
                }
                std::thread::sleep(Duration::from_millis(25));
            };
            let output = tail(format!("{}{}", ho.join().unwrap_or_default(), he.join().unwrap_or_default()));
            let mut g = me.inner.lock().unwrap();
            let Some((j, slot)) = g.get_mut(&jid) else { return };
            *slot = None;
            j.output = output;
            j.exit_code = status.and_then(|s| s.code());
            j.finished_at = Some(spide_core::store::now_ms());
            if j.state == JobState::Running {
                j.state = if timed_out {
                    JobState::TimedOut
                } else if j.exit_code == Some(0) {
                    JobState::Succeeded
                } else {
                    JobState::Failed
                };
            }
            let snapshot = j.clone();
            drop(g);
            me.persist(&snapshot);
            done(&snapshot);
        });
        Ok(id)
    }

    /// Run `f` on a thread as a job. `f` gets a cancel flag; its `Ok` text becomes the output.
    pub fn spawn_task(&self, kind: &str, label: &str, op: Option<String>, f: impl FnOnce(&std::sync::atomic::AtomicBool) -> Result<String, String> + Send + 'static) -> String {
        let id = self.next_id();
        let job = Job { id: id.clone(), kind: kind.into(), label: label.into(), state: JobState::Running, started_at: spide_core::store::now_ms(), finished_at: None, exit_code: None, output: String::new(), op };
        self.persist(&job);
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.inner.lock().unwrap().insert(id.clone(), (job, None));
        self.flags.lock().unwrap().insert(id.clone(), cancel.clone());
        let me = self.clone();
        let jid = id.clone();
        std::thread::spawn(move || {
            let r = f(&cancel);
            let mut g = me.inner.lock().unwrap();
            let Some((j, _)) = g.get_mut(&jid) else { return };
            j.finished_at = Some(spide_core::store::now_ms());
            match r {
                Ok(out) => {
                    j.output = tail(out);
                    if j.state == JobState::Running {
                        j.state = JobState::Succeeded;
                    }
                }
                Err(e) => {
                    j.output = tail(e);
                    if j.state == JobState::Running {
                        j.state = JobState::Failed;
                    }
                }
            }
            let snap = j.clone();
            drop(g);
            me.flags.lock().unwrap().remove(&jid);
            me.persist(&snap);
        });
        id
    }

    pub fn get(&self, id: &str) -> Option<Job> {
        self.inner.lock().unwrap().get(id).map(|(j, _)| j.clone())
    }

    pub fn list(&self) -> Vec<Job> {
        self.inner.lock().unwrap().values().map(|(j, _)| j.clone()).collect()
    }

    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let mut g = self.inner.lock().unwrap();
        let (j, child) = g.get_mut(id).ok_or_else(|| format!("unknown job {id}"))?;
        if j.state != JobState::Running {
            return Err(format!("job {id} is already {:?}", j.state));
        }
        j.state = JobState::Cancelled;
        if let Some(c) = child {
            let _ = c.lock().unwrap().kill();
        }
        if let Some(f) = self.flags.lock().unwrap().get(id) {
            f.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }

    /// Block until the job leaves `Running` (tests, synchronous callers).
    pub fn wait(&self, id: &str, max: Duration) -> Option<Job> {
        let start = Instant::now();
        loop {
            let j = self.get(id)?;
            if j.state != JobState::Running && j.finished_at.is_some() {
                return Some(j);
            }
            if start.elapsed() > max {
                return Some(j);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_failure_timeout_cancel() {
        let d = tempfile::tempdir().unwrap();
        let j = Jobs::new(d.path());
        let ok = j.spawn_command("check", "t", "echo hi", d.path(), Duration::from_secs(5), None, |_| {}).unwrap();
        let bad = j.spawn_command("check", "t", "echo no >&2; exit 3", d.path(), Duration::from_secs(5), None, |_| {}).unwrap();
        let slow = j.spawn_command("check", "t", "sleep 5", d.path(), Duration::from_millis(100), None, |_| {}).unwrap();
        let cancel = j.spawn_command("check", "t", "sleep 5", d.path(), Duration::from_secs(10), None, |_| {}).unwrap();
        j.cancel(&cancel).unwrap();
        let w = |id: &str| j.wait(id, Duration::from_secs(5)).unwrap();
        assert_eq!(w(&ok).state, JobState::Succeeded);
        assert!(w(&ok).output.contains("hi"));
        assert_eq!((w(&bad).state, w(&bad).exit_code), (JobState::Failed, Some(3)));
        assert_eq!(w(&slow).state, JobState::TimedOut);
        assert_eq!(w(&cancel).state, JobState::Cancelled);
    }

    #[test]
    fn running_jobs_of_a_dead_process_are_interrupted() {
        let d = tempfile::tempdir().unwrap();
        let j = Jobs::new(d.path());
        j.persist(&Job { id: "J1".into(), kind: "check".into(), label: "x".into(), state: JobState::Running, started_at: 0, finished_at: None, exit_code: None, output: String::new(), op: None });
        let j2 = Jobs::new(d.path());
        assert_eq!(j2.recover(), vec!["J1".to_string()]);
        assert_eq!(j2.get("J1").unwrap().state, JobState::Interrupted);
    }
}
