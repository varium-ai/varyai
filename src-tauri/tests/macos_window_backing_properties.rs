#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    use std::mem;

    use objc2::{msg_send, sel, ClassType};
    use objc2_app_kit::{NSWindow, NSWindowDidChangeBackingPropertiesNotification};
    use objc2_foundation::NSNotification;
    use tao::{
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        platform::{
            macos::{ActivationPolicy, EventLoopExtMacOS, WindowExtMacOS},
            run_return::EventLoopExtRunReturn,
        },
        window::WindowBuilder,
    };

    const TEST_SCALE: f64 = 3.0;

    unsafe extern "C-unwind" fn backing_scale_factor(_: &NSWindow, _: objc2::runtime::Sel) -> f64 {
        TEST_SCALE
    }

    // AppKit and Tao require a real main thread, so this test has its own executable.
    let mut event_loop = EventLoop::new();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let window = WindowBuilder::new()
        .with_visible(false)
        .build(&event_loop)
        .expect("hidden fixture window");
    let native_window = unsafe { &*window.ns_window().cast::<NSWindow>() };
    let delegate = native_window.delegate().expect("Tao window delegate");
    let view = native_window.contentView().expect("fixture content view");
    let notification = unsafe {
        NSNotification::notificationWithName_object(
            NSWindowDidChangeBackingPropertiesNotification,
            Some(native_window),
        )
    };

    // Simulate a scale change without switching the user's display configuration.
    assert_ne!(window.scale_factor(), TEST_SCALE);
    let method = NSWindow::class()
        .instance_method(sel!(backingScaleFactor))
        .expect("AppKit backing scale method");
    unsafe {
        native_window.setContentView(None);
        assert!(native_window.contentView().is_none());

        let original = method.set_implementation(mem::transmute::<
            unsafe extern "C-unwind" fn(&NSWindow, objc2::runtime::Sel) -> f64,
            objc2::runtime::Imp,
        >(backing_scale_factor));
        let _: () = msg_send![&*delegate, windowDidChangeBackingProperties: &*notification];
        let _: () = msg_send![&*delegate, windowDidResize: &*notification];
        native_window.setContentView(Some(&view));
        let _: () = msg_send![&*delegate, windowDidChangeBackingProperties: &*notification];
        method.set_implementation(original);
    }

    let mut scale_events = 0;
    event_loop.run_return(|event, _, flow| match event {
        Event::WindowEvent {
            window_id,
            event: WindowEvent::ScaleFactorChanged { scale_factor, .. },
            ..
        } if window_id == window.id() && scale_factor == TEST_SCALE => scale_events += 1,
        Event::MainEventsCleared => *flow = ControlFlow::Exit,
        _ => (),
    });
    assert_eq!(
        scale_events, 1,
        "the restored view receives the scale change"
    );
}
