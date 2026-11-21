//! Finder file-open and Dock reopen/Quit adapter. Install after Winit builds its event
//! loop but before AppKit starts dispatching launch events.
#[cfg(target_os = "macos")]
mod mac {
    use crate::events::{Events, MenuAction, UiEvent};
    use objc2::{define_class, ffi, sel, ClassType, MainThreadOnly};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_app_kit::{NSApplication, NSApplicationDelegate, NSApplicationTerminateReply};
    use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSArray, NSURL};
    use std::sync::OnceLock;
    static EVENTS: OnceLock<Events> = OnceLock::new();
    define_class!(
        // SAFETY: NSObject has no subclassing invariants. This class supplies method
        // metadata only; the callbacks never inspect the receiver or its ivars.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        struct SpideDelegate;
        unsafe impl NSObjectProtocol for SpideDelegate {}
        unsafe impl NSApplicationDelegate for SpideDelegate {
            // SAFETY: signatures match NSApplicationDelegate.
            #[unsafe(method(application:openURLs:))]
            fn open_urls(_this: *const Self, _app: &NSApplication, urls: &NSArray<NSURL>) {
                let paths = urls.iter().filter(|u| u.isFileURL()).filter_map(|u| u.to_file_path()).collect();
                if let Some(events) = EVENTS.get() { events.send(UiEvent::OpenPaths(paths)); }
            }
            #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
            fn reopen(_this: *const Self, _app: &NSApplication, _visible: bool) -> bool {
                if let Some(events) = EVENTS.get() { events.send(UiEvent::RaiseWindow); }
                true
            }
            #[unsafe(method(applicationShouldTerminate:))]
            fn should_terminate(_this: *const Self, _app: &NSApplication) -> NSApplicationTerminateReply {
                // Dock Quit must use the same leave guard as the File/app menu, not force exit.
                if let Some(events) = EVENTS.get() { events.send(UiEvent::Menu(MenuAction::Quit)); }
                NSApplicationTerminateReply::TerminateCancel
            }
        }
    );
    pub fn install(events: &Events) {
        if let Err(error) = install_callbacks(events) {
            eprintln!("spide: Finder open integration not installed: {error}");
        }
    }

    fn install_callbacks(events: &Events) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("expected the main thread")?;
        let app = NSApplication::sharedApplication(mtm);
        let delegate = app.delegate().ok_or("Winit application delegate is missing")?;
        let object: &AnyObject = (*delegate).as_ref();
        let class = object.class();
        if class.name() != c"WinitApplicationDelegate" {
            return Err(format!("unexpected application delegate class {}", class.name().to_string_lossy()));
        }

        let mut methods = Vec::new();
        for selector in [
            sel!(application:openURLs:),
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            sel!(applicationShouldTerminate:),
        ] {
            if class.instance_method(selector).is_some() {
                return Err(format!("Winit delegate already handles {}", selector.name().to_string_lossy()));
            }
            let method = SpideDelegate::class().instance_method(selector)
                .ok_or_else(|| format!("Spide callback metadata missing for {}", selector.name().to_string_lossy()))?;
            methods.push(method);
        }
        EVENTS.set(events.clone()).map_err(|_| "callback event queue already installed")?;
        for method in methods {
            // SAFETY: The Objective-C runtime adds (never replaces) the preflighted
            // selector. Its implementation and full ABI come from the typed AppKit
            // method on SpideDelegate. The implementation does not use its receiver.
            let added = unsafe {
                ffi::class_addMethod(
                    std::ptr::from_ref::<AnyClass>(class).cast_mut(),
                    method.name(),
                    method.implementation(),
                    ffi::method_getTypeEncoding(std::ptr::from_ref(method)),
                )
            };
            if !added.as_bool() {
                return Err(format!("could not add {} to Winit delegate", method.name().name().to_string_lossy()));
            }
        }
        // AppKit may cache which optional methods its delegate implements. Refresh
        // registration without changing the delegate object or Winit's lifecycle.
        app.setDelegate(None);
        app.setDelegate(Some(&delegate));
        Ok(())
    }
}
#[cfg(target_os = "macos")]
pub use mac::install;
