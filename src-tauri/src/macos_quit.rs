use objc2::{
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel, MainThreadMarker,
};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
use std::sync::{Once, OnceLock};

static REQUEST_EXIT: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
static QUIT_REQUESTED: Once = Once::new();

pub(crate) fn install(app: &tauri::AppHandle) -> Result<(), String> {
    let main_thread =
        MainThreadMarker::new().ok_or("Native quit setup requires the main thread")?;
    let application = NSApplication::sharedApplication(main_thread);
    let delegate = application
        .delegate()
        .ok_or("Application delegate is unavailable")?;
    let delegate_object: &AnyObject = (*delegate).as_ref();
    let app = app.clone();
    install_on_delegate(delegate_object.class(), move || {
        log::info!("收到 macOS 原生退出请求，转入配置恢复流程");
        app.exit(0);
    })
}

extern "C-unwind" fn application_should_terminate(
    _: &AnyObject,
    _: Sel,
    _: &AnyObject,
) -> NSApplicationTerminateReply {
    QUIT_REQUESTED.call_once(|| {
        if let Some(request_exit) = REQUEST_EXIT.get() {
            request_exit();
        }
    });
    // Keep AppKit from terminating before the existing asynchronous cleanup finishes.
    NSApplicationTerminateReply::TerminateCancel
}

fn install_on_delegate(
    class: &AnyClass,
    request_exit: impl Fn() + Send + Sync + 'static,
) -> Result<(), String> {
    let selector = sel!(applicationShouldTerminate:);
    // Do not replace a termination policy supplied by the runtime or another plugin.
    if class.instance_method(selector).is_some() {
        return Err("Application delegate already handles native quit".into());
    }
    REQUEST_EXIT
        .set(Box::new(request_exit))
        .map_err(|_| "Native quit handler is already installed")?;
    // SAFETY: AppKit's method returns NSUInteger and accepts self, _cmd, NSApplication*.
    // macOS targets are 64-bit (Q). No ivars, delegate identity, or other methods change.
    let added = unsafe {
        let implementation = std::mem::transmute::<
            extern "C-unwind" fn(&AnyObject, Sel, &AnyObject) -> NSApplicationTerminateReply,
            Imp,
        >(application_should_terminate);
        objc2::ffi::class_addMethod(
            (class as *const AnyClass).cast_mut(),
            selector,
            implementation,
            c"Q@:@".as_ptr(),
        )
    };
    if added.as_bool() {
        Ok(())
    } else {
        Err("Cannot install native quit handler".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::{msg_send, rc::Retained, runtime::ClassBuilder, sel, ClassType};
    use objc2_app_kit::NSApplicationTerminateReply;
    use objc2_foundation::NSObject;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn native_quit_cancels_immediate_termination_and_requests_restoring_exit() {
        let class = ClassBuilder::new(c"VaryAIQuitTestDelegate", NSObject::class())
            .expect("test delegate class")
            .register();
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        install_on_delegate(class, move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .expect("install native quit handler");
        assert!(class
            .instance_method(sel!(applicationShouldTerminate:))
            .is_some());
        let delegate: Retained<NSObject> = unsafe { msg_send![class, new] };
        let sender = NSObject::new();
        let reply: NSApplicationTerminateReply =
            unsafe { msg_send![&delegate, applicationShouldTerminate: &*sender] };
        assert_eq!(reply, NSApplicationTerminateReply::TerminateCancel);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        let second_reply: NSApplicationTerminateReply =
            unsafe { msg_send![&delegate, applicationShouldTerminate: &*sender] };
        assert_eq!(second_reply, NSApplicationTerminateReply::TerminateCancel);
        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "cleanup must not run twice"
        );
    }

    #[test]
    fn preserves_existing_delegate_termination_handler() {
        extern "C-unwind" fn existing(
            _: &AnyObject,
            _: Sel,
            _: &AnyObject,
        ) -> NSApplicationTerminateReply {
            NSApplicationTerminateReply::TerminateNow
        }
        let mut class = ClassBuilder::new(c"VaryAIExistingQuitTestDelegate", NSObject::class())
            .expect("test delegate class");
        unsafe {
            class.add_method(
                sel!(applicationShouldTerminate:),
                existing as extern "C-unwind" fn(_, _, _) -> _,
            );
        }
        let class = class.register();
        assert!(
            install_on_delegate(class, || panic!("must not replace existing handler")).is_err()
        );
        let delegate: Retained<NSObject> = unsafe { msg_send![class, new] };
        let sender = NSObject::new();
        let reply: NSApplicationTerminateReply =
            unsafe { msg_send![&delegate, applicationShouldTerminate: &*sender] };
        assert_eq!(reply, NSApplicationTerminateReply::TerminateNow);
    }
}
