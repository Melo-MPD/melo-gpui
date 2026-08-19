//! Sleep/wake notifications from NSWorkspace, delivered on the MPD event
//! channel so `AppState` can re-establish sessions and re-open hogged DACs.

use crate::mpd::client::MpdEvent;
use futures::channel::mpsc::UnboundedSender;

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
pub fn start(events: UnboundedSender<MpdEvent>) {
    use block::ConcreteBlock;
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::c_void;

    unsafe fn ns_string(s: &str) -> *mut Object {
        let cls = class!(NSString);
        let bytes = s.as_ptr() as *const c_void;
        let obj: *mut Object = msg_send![cls, alloc];
        // 4 = NSUTF8StringEncoding
        let obj: *mut Object = msg_send![obj, initWithBytes: bytes length: s.len() encoding: 4u64];
        obj
    }

    unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let center: *mut Object = msg_send![workspace, notificationCenter];
        let nil: *mut Object = std::ptr::null_mut();

        let pairs: [(&str, fn() -> MpdEvent); 2] = [
            ("NSWorkspaceDidWakeNotification", || MpdEvent::SystemWoke),
            ("NSWorkspaceWillSleepNotification", || {
                MpdEvent::SystemWillSleep
            }),
        ];
        for (name, make) in pairs {
            let tx = events.clone();
            let block = ConcreteBlock::new(move |_note: *mut Object| {
                let _ = tx.unbounded_send(make());
            })
            .copy();
            let name_obj = ns_string(name);
            // The observer token is intentionally leaked: it lives for the process.
            let _token: *mut Object = msg_send![center,
                addObserverForName: name_obj
                object: nil
                queue: nil
                usingBlock: &*block];
            std::mem::forget(block);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn start(_events: UnboundedSender<MpdEvent>) {}
