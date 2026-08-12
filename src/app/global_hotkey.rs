//! Native macOS key monitor that keeps Cmd-` cycling workspaces even while a
//! Vietnamese IME composition is open.
//!
//! While a text input holds a composing syllable, `gpui_macos` routes every
//! keystroke - Cmd-modified ones included - through the input method first
//! (its `is_composing` branch). The Vietnamese input source then swallows the
//! grave key, so the `NextWorkspace` key binding never fires until the
//! composition ends. Switching workspace by clicking clears it, which is the
//! "click a workspace and it works again" symptom.
//!
//! A local `NSEvent` monitor runs inside `-[NSApplication sendEvent:]`, before
//! the event reaches the focused view's input context, so it sees Cmd-`
//! regardless of composition state. It dispatches the action and swallows the
//! event, which also stops the macOS "Move focus to next window" shortcut from
//! claiming the same key.

#[cfg(target_os = "macos")]
mod macos {
    use std::ptr;
    use std::ptr::NonNull;
    use std::sync::OnceLock;

    use async_channel::Sender;
    use block2::RcBlock;
    use gpui::App;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};

    use crate::app::shell::NextWorkspace;

    /// Physical key code for the grave/backtick key. Layout independent, so the
    /// match still holds under a dead-key or non-QWERTY layout.
    const GRAVE_KEY_CODE: u16 = 50;

    static SENDER: OnceLock<Sender<()>> = OnceLock::new();

    pub fn init(cx: &mut App) {
        if SENDER.get().is_some() {
            return;
        }
        let (sender, receiver) = async_channel::unbounded::<()>();
        if SENDER.set(sender).is_err() {
            return;
        }

        let handler = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit hands the monitor a live key-down event for the
            // duration of the call.
            let event_ref = unsafe { event.as_ref() };
            if is_next_workspace_chord(event_ref) {
                if let Some(sender) = SENDER.get() {
                    let _ = sender.try_send(());
                }
                // Swallow it: neither the IME nor the window-cycle shortcut
                // should see Cmd-`.
                return ptr::null_mut();
            }
            event.as_ptr()
        });

        // SAFETY: the block matches the documented handler signature and returns
        // either the original event or null.
        let _monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &handler)
        };

        cx.spawn(async move |cx| {
            while receiver.recv().await.is_ok() {
                let _ = cx.update(|cx| cx.dispatch_action(&NextWorkspace));
            }
        })
        .detach();
    }

    /// True for a bare Cmd-` press. Shift, Control and Option must be absent so
    /// Cmd-Shift-` and other chords keep their normal handling.
    fn is_next_workspace_chord(event: &NSEvent) -> bool {
        // SAFETY: reading fields of a live key event is always valid.
        if unsafe { event.keyCode() } != GRAVE_KEY_CODE {
            return false;
        }
        let flags = unsafe { event.modifierFlags() };
        let others = NSEventModifierFlags::Shift
            | NSEventModifierFlags::Control
            | NSEventModifierFlags::Option;
        flags.contains(NSEventModifierFlags::Command) && !flags.intersects(others)
    }
}

#[cfg(target_os = "macos")]
pub use macos::init;

#[cfg(not(target_os = "macos"))]
pub fn init(_: &mut gpui::App) {}
