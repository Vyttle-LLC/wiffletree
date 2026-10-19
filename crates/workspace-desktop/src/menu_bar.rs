//! The menu bar icon while the app runs in the background: quiet while no agent works, the count
//! beside it while any does, and a menu to come back, stop the agents or quit. GPUI has no status
//! items, so this is AppKit's `NSStatusItem`, used on the main thread only.
// objc 0.2's macros test a `cargo-clippy` feature this crate does not declare.
#![allow(unexpected_cfgs)]
use objc::{class, declare::ClassDecl, msg_send, runtime::Object, runtime::Sel, sel, sel_impl};
use std::{
    ffi::CString,
    sync::{Mutex, Once},
};

/// What the human chose from the icon's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Open,
    StopAgentsAndQuit,
    Quit,
}

/// What the icon shows beside its symbol: nothing while no agent works.
pub(crate) fn badge(working: usize) -> Option<String> {
    (working > 0).then(|| working.to_string())
}

/// The menu's first line.
pub(crate) fn summary(working: usize) -> String {
    match working {
        0 => "No agents working".into(),
        1 => "1 agent working".into(),
        n => format!("{n} agents working"),
    }
}

/// Where the menu's items send the human's choice.
static CHOICES: Mutex<Option<async_channel::Sender<Choice>>> = Mutex::new(None);

fn choose(choice: Choice) {
    if let Some(choices) = CHOICES.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = choices.try_send(choice);
    }
}
extern "C" fn open(_: &Object, _: Sel, _: *mut Object) {
    choose(Choice::Open)
}
extern "C" fn stop_agents_and_quit(_: &Object, _: Sel, _: *mut Object) {
    choose(Choice::StopAgentsAndQuit)
}
extern "C" fn quit(_: &Object, _: Sel, _: *mut Object) {
    choose(Choice::Quit)
}

/// The class whose instance the menu items call.
fn target_class() -> &'static objc::runtime::Class {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        let mut class = ClassDecl::new("WiffletreeMenuBarTarget", class!(NSObject))
            .expect("the class is declared once");
        // SAFETY: each method has the signature of an action: self, its selector, the sender.
        unsafe {
            let action = open as extern "C" fn(&Object, Sel, *mut Object);
            class.add_method(sel!(openWiffletree:), action);
            let action = stop_agents_and_quit as extern "C" fn(&Object, Sel, *mut Object);
            class.add_method(sel!(stopAgentsAndQuit:), action);
            let action = quit as extern "C" fn(&Object, Sel, *mut Object);
            class.add_method(sel!(quitWiffletree:), action);
        }
        class.register();
    });
    class!(WiffletreeMenuBarTarget)
}

/// An autoreleased `NSString`.
fn string(text: &str) -> *mut Object {
    let text = CString::new(text).expect("no NUL in menu text");
    // SAFETY: a valid C string, copied by NSString.
    unsafe { msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()] }
}

/// The status item and its menu, removed from the menu bar when dropped.
pub(crate) struct Icon {
    item: *mut Object,
    summary: *mut Object,
    target: *mut Object,
}
impl Icon {
    /// Shows the icon, sending what the human chooses from its menu to `choices`.
    pub(crate) fn show(choices: async_channel::Sender<Choice>) -> Self {
        *CHOICES.lock().unwrap_or_else(|e| e.into_inner()) = Some(choices);
        // SAFETY: on the main thread; every object created here is retained by the item, the
        // menu or this struct, which releases its own references when dropped.
        unsafe {
            let bar: *mut Object = msg_send![class!(NSStatusBar), systemStatusBar];
            // NSVariableStatusItemLength
            let item: *mut Object = msg_send![bar, statusItemWithLength: -1.0f64];
            let _: () = msg_send![item, retain];
            let button: *mut Object = msg_send![item, button];
            let mut image: *mut Object = msg_send![class!(NSImage),
                imageWithSystemSymbolName: string("tree")
                accessibilityDescription: string("Wiffletree")];
            if image.is_null() {
                image = msg_send![class!(NSImage),
                    imageWithSystemSymbolName: string("leaf")
                    accessibilityDescription: string("Wiffletree")];
            }
            let _: () = msg_send![image, setTemplate: true];
            let _: () = msg_send![button, setImage: image];
            // NSImageLeft: the count follows the symbol.
            let _: () = msg_send![button, setImagePosition: 2usize];

            let target: *mut Object = msg_send![target_class(), new];
            let menu: *mut Object = msg_send![class!(NSMenu), new];
            let summary = Self::add(menu, &summary(0), None, target);
            let separator: *mut Object = msg_send![class!(NSMenuItem), separatorItem];
            let _: () = msg_send![menu, addItem: separator];
            Self::add(menu, "Open Wiffletree", Some(sel!(openWiffletree:)), target);
            let stop = sel!(stopAgentsAndQuit:);
            Self::add(menu, "Stop agents and quit", Some(stop), target);
            Self::add(menu, "Quit Wiffletree", Some(sel!(quitWiffletree:)), target);
            let _: () = msg_send![item, setMenu: menu];
            let _: () = msg_send![menu, release];
            let icon = Self {
                item,
                summary,
                target,
            };
            icon.set_working(0);
            icon
        }
    }
    /// Adds an item to `menu`; one without an action is disabled. Returns it, still owned by
    /// the menu.
    unsafe fn add(
        menu: *mut Object,
        title: &str,
        action: Option<Sel>,
        target: *mut Object,
    ) -> *mut Object {
        // SAFETY: as in `show`; the menu retains the item, which is then released once here.
        unsafe {
            let item: *mut Object = msg_send![class!(NSMenuItem), alloc];
            let action = action.unwrap_or(Sel::from_ptr(std::ptr::null()));
            let item: *mut Object = msg_send![item,
                initWithTitle: string(title)
                action: action
                keyEquivalent: string("")];
            let _: () = msg_send![item, setTarget: target];
            let _: () = msg_send![menu, addItem: item];
            let _: () = msg_send![item, release];
            item
        }
    }
    /// Shows `working` agents: the count beside the symbol, or nothing while none works.
    pub(crate) fn set_working(&self, working: usize) {
        let badge = badge(working).map_or(String::new(), |count| format!(" {count}"));
        // SAFETY: on the main thread, with the item and its menu alive.
        unsafe {
            let button: *mut Object = msg_send![self.item, button];
            let _: () = msg_send![button, setTitle: string(&badge)];
            let _: () = msg_send![self.summary, setTitle: string(&summary(working))];
        }
    }
}
impl Drop for Icon {
    fn drop(&mut self) {
        // SAFETY: on the main thread; the item and target were retained in `show`.
        unsafe {
            let bar: *mut Object = msg_send![class!(NSStatusBar), systemStatusBar];
            let _: () = msg_send![bar, removeStatusItem: self.item];
            let _: () = msg_send![self.item, release];
            let _: () = msg_send![self.target, release];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_quiet_while_no_agent_works_and_counts_them_otherwise() {
        assert_eq!(badge(0), None);
        assert_eq!(badge(1).as_deref(), Some("1"));
        assert_eq!(summary(0), "No agents working");
        assert_eq!(summary(1), "1 agent working");
        assert_eq!(summary(3), "3 agents working");
    }
}
