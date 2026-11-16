//! Finder file-open and Dock reopen/Quit adapter. Winit explicitly leaves NSApplicationDelegate
//! available to applications. Install after the event loop exists; never replace another delegate.
#[cfg(target_os = "macos")]
mod mac {
    use crate::events::{Events, MenuAction, UiEvent};
    use objc2::{define_class, msg_send, MainThreadOnly};
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSApplication, NSApplicationDelegate, NSApplicationTerminateReply};
    use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSArray, NSURL};
    use std::{cell::RefCell, sync::OnceLock};
    static EVENTS: OnceLock<Events> = OnceLock::new();
    define_class!(
        // SAFETY: NSObject has no subclassing invariants; the delegate is main-thread-only.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        struct SpideDelegate;
        unsafe impl NSObjectProtocol for SpideDelegate {}
        unsafe impl NSApplicationDelegate for SpideDelegate {
            // SAFETY: signatures match NSApplicationDelegate.
            #[unsafe(method(application:openURLs:))]
            fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
                let paths = urls.iter().filter(|u| u.isFileURL()).filter_map(|u| u.to_file_path()).collect();
                if let Some(events) = EVENTS.get() { events.send(UiEvent::OpenPaths(paths)); }
            }
            #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
            fn reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
                if let Some(events) = EVENTS.get() { events.send(UiEvent::RaiseWindow); }
                true
            }
            #[unsafe(method(applicationShouldTerminate:))]
            fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
                // Dock Quit must use the same leave guard as the File/app menu, not force exit.
                if let Some(events) = EVENTS.get() { events.send(UiEvent::Menu(MenuAction::Quit)); }
                NSApplicationTerminateReply::TerminateCancel
            }
        }
    );
    thread_local! { static DELEGATE: RefCell<Option<Retained<SpideDelegate>>> = const { RefCell::new(None) }; }
    pub fn install(events: &Events) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        if app.delegate().is_some() {
            events.send(UiEvent::Toast { kind: "error".into(), text: "Finder open integration not installed: another application delegate is present.".into() });
            return;
        }
        let _ = EVENTS.set(events.clone());
        // SAFETY: NSObject init returns the allocated subclass and is called on the main thread.
        let delegate: Retained<SpideDelegate> = unsafe { msg_send![SpideDelegate::alloc(mtm), init] };
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        DELEGATE.with(|d| *d.borrow_mut() = Some(delegate));
    }
}
#[cfg(target_os = "macos")]
pub use mac::install;
#[cfg(not(target_os = "macos"))]
pub fn install(_: &crate::events::Events) {}
