use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Once;

use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use cocoa_foundation::foundation::NSRange;
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{msg_send, sel, sel_impl};

#[cfg(target_os = "macos")]
use crate::bridge::paste_text_and_restore_clipboard;
use crate::bridge::MacImeBridge;
use voice_input_core::Result;

static ACTIVE_CONTROLLER: AtomicPtr<Object> = AtomicPtr::new(ptr::null_mut());
static REGISTER_CONTROLLER: Once = Once::new();
static REGISTER_EXTENSION_DELEGATE: Once = Once::new();

#[cfg(target_os = "macos")]
pub fn register_input_controller_class() {
    REGISTER_CONTROLLER.call_once(|| unsafe {
        let superclass =
            Class::get("IMKInputController").expect("InputMethodKit 未提供 IMKInputController");
        let mut decl =
            ClassDecl::new("VoiceInputInputController", superclass).expect("注册输入法控制器类");
        decl.add_ivar::<*mut Object>("rustOwner");
        decl.add_method(
            sel!(initWithServer:delegate:client:),
            voiceinput_init as extern "C" fn(&mut Object, Sel, id, id, id) -> id,
        );
        decl.add_method(
            sel!(dealloc),
            voiceinput_dealloc as extern "C" fn(&mut Object, Sel),
        );
        decl.register();
    });

    register_extension_delegate_class();
}

#[cfg(not(target_os = "macos"))]
pub fn register_input_controller_class() {}

#[cfg(target_os = "macos")]
fn register_extension_delegate_class() {
    REGISTER_EXTENSION_DELEGATE.call_once(|| {
        let superclass = Class::get("NSObject").expect("Objective-C 未提供 NSObject");
        let decl = ClassDecl::new("VoiceInputExtensionDelegate", superclass)
            .expect("注册输入法扩展代理类");
        decl.register();
    });
}

#[cfg(target_os = "macos")]
extern "C" fn voiceinput_init(
    this: &mut Object,
    _cmd: Sel,
    server: id,
    delegate: id,
    client: id,
) -> id {
    unsafe {
        let superclass =
            Class::get("IMKInputController").expect("InputMethodKit 未提供 IMKInputController");
        let initialized: id = msg_send![super(this, superclass), initWithServer:server delegate:delegate client:client];
        if initialized != nil {
            set_active_controller(initialized);
        }
        initialized
    }
}

#[cfg(target_os = "macos")]
extern "C" fn voiceinput_dealloc(this: &mut Object, _cmd: Sel) {
    clear_active_controller();
    unsafe {
        let superclass =
            Class::get("IMKInputController").expect("InputMethodKit 未提供 IMKInputController");
        let _: () = msg_send![super(this, superclass), dealloc];
    }
}

#[cfg(target_os = "macos")]
pub fn set_active_controller(controller: id) {
    let ptr = controller as *mut Object;
    if ptr.is_null() {
        return;
    }

    unsafe {
        let _: id = msg_send![controller, retain];
    }

    let old = ACTIVE_CONTROLLER.swap(ptr, Ordering::SeqCst);
    if !old.is_null() {
        unsafe {
            let _: () = msg_send![old, release];
        }
    }
}

#[cfg(target_os = "macos")]
pub fn clear_active_controller() {
    let old = ACTIVE_CONTROLLER.swap(ptr::null_mut(), Ordering::SeqCst);
    if !old.is_null() {
        unsafe {
            let _: () = msg_send![old, release];
        }
    }
}

#[cfg(target_os = "macos")]
pub fn has_active_controller() -> bool {
    !ACTIVE_CONTROLLER.load(Ordering::SeqCst).is_null()
}

#[cfg(target_os = "macos")]
fn active_controller() -> Option<id> {
    let ptr = ACTIVE_CONTROLLER.load(Ordering::SeqCst);
    if ptr.is_null() {
        None
    } else {
        Some(ptr as id)
    }
}

#[cfg(target_os = "macos")]
pub struct InputMethodKitMacImeBridge;

#[cfg(target_os = "macos")]
impl Default for InputMethodKitMacImeBridge {
    fn default() -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
impl MacImeBridge for InputMethodKitMacImeBridge {
    fn start_composition(&self) -> Result<()> {
        Ok(())
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        if let Some(controller) = active_controller() {
            unsafe {
                let string = NSString::alloc(nil).init_str(text);
                let caret = text.encode_utf16().count() as u64;
                let range = NSRange::new(caret, 0);
                let _: () = msg_send![controller, setMarkedText:string selectedRange:range replacementRange:NSRange::new(0, 0)];
            }
        }
        Ok(())
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        if let Some(controller) = active_controller() {
            unsafe {
                let string = NSString::alloc(nil).init_str(text);
                let range = NSRange::new(0, 0);
                let _: () = msg_send![controller, insertText:string replacementRange:range];
            }
            return Ok(());
        }

        paste_text_and_restore_clipboard(text)
    }

    fn cancel_composition(&self) -> Result<()> {
        if let Some(controller) = active_controller() {
            unsafe {
                let _: () = msg_send![controller, unmarkText];
            }
        }
        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
pub struct InputMethodKitMacImeBridge;

#[cfg(not(target_os = "macos"))]
impl Default for InputMethodKitMacImeBridge {
    fn default() -> Self {
        Self
    }
}

#[cfg(not(target_os = "macos"))]
impl MacImeBridge for InputMethodKitMacImeBridge {
    fn start_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "InputMethodKit 桥接只支持 macOS".to_string(),
        ))
    }

    fn update_preedit(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "InputMethodKit 桥接只支持 macOS".to_string(),
        ))
    }

    fn commit_text(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "InputMethodKit 桥接只支持 macOS".to_string(),
        ))
    }

    fn cancel_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "InputMethodKit 桥接只支持 macOS".to_string(),
        ))
    }

    fn end_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "InputMethodKit 桥接只支持 macOS".to_string(),
        ))
    }
}
