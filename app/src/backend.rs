//! The UI's handle on the backend: blocking commands run on tokio's blocking pool and post their
//! results back to the UI thread (errors become toasts, like `run()` in the old React app).

use crate::events::Events;
use crate::pi_bridge::PiBridge;
use crate::state::AppState;
use crate::ui::app::SpideApp;
use std::sync::Arc;

#[derive(Clone)]
pub struct Backend {
    pub st: AppState,
    pub pi: Arc<PiBridge>,
    pub events: Events,
    rt: Option<tokio::runtime::Handle>,
}

impl Backend {
    pub fn new(st: AppState, pi: Arc<PiBridge>, rt: Option<tokio::runtime::Handle>) -> Self {
        let events = st.events.clone();
        Self { st, pi, events, rt }
    }

    fn spawn(&self, job: impl FnOnce() + Send + 'static) {
        match &self.rt {
            Some(rt) => {
                rt.spawn_blocking(job);
            }
            // tests: run inline
            None => job(),
        }
    }

    /// Run `f` off the UI thread; `then` gets the result on the UI thread.
    /// Scoped to the current workspace epoch: if the project is switched or closed before `f`
    /// starts, `f` never runs; if it changes while `f` runs, the result is dropped.
    pub fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&AppState, &PiBridge) -> Result<T, String> + Send + 'static,
        then: impl FnOnce(&mut SpideApp, Result<T, String>) + Send + 'static,
    ) {
        let epoch = self.st.epoch();
        let (st, pi, ev) = (self.st.clone(), self.pi.clone(), self.events.clone());
        self.spawn(move || {
            if st.epoch() != epoch {
                return;
            }
            let r = f(&st, &pi);
            if st.epoch() != epoch {
                return;
            }
            ev.task(move |app| {
                if app.backend.st.epoch() == epoch {
                    then(app, r)
                }
            });
        });
    }

    /// A workspace transition (open/new/close): it changes the epoch itself, so its result is
    /// always delivered. It still refuses to start if another transition already happened.
    pub fn transition<T: Send + 'static>(
        &self,
        f: impl FnOnce(&AppState, &PiBridge) -> Result<T, String> + Send + 'static,
        then: impl FnOnce(&mut SpideApp, Result<T, String>) + Send + 'static,
    ) {
        let epoch = self.st.epoch();
        let (st, pi, ev) = (self.st.clone(), self.pi.clone(), self.events.clone());
        self.spawn(move || {
            let r = if st.epoch() != epoch { Err("The project changed before this request started; nothing was done.".to_string()) } else { f(&st, &pi) };
            ev.task(move |app| then(app, r));
        });
    }

    /// Fire and forget; an error becomes a toast.
    pub fn run(&self, f: impl FnOnce(&AppState, &PiBridge) -> Result<(), String> + Send + 'static) {
        self.call(f, |app, r| {
            if let Err(e) = r {
                app.toast("error", e);
            }
        });
    }

    /// Like `run`, then `ok` on success only.
    pub fn run_then<T: Send + 'static>(
        &self,
        f: impl FnOnce(&AppState, &PiBridge) -> Result<T, String> + Send + 'static,
        ok: impl FnOnce(&mut SpideApp, T) + Send + 'static,
    ) {
        self.call(f, |app, r| match r {
            Ok(v) => ok(app, v),
            Err(e) => app.toast("error", e),
        });
    }
}
