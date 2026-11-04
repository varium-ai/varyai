//! AppKit points stay in one screen coordinate system, including mixed-DPI displays.
use objc2::{define_class, msg_send, rc::Retained, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSColor, NSEvent, NSPanel, NSScreen, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::RefCell;
use tauri::{Emitter, Manager};

define_class!(
    // SAFETY: Only main-thread AppKit code creates this panel. No ivars are added.
    #[unsafe(super = NSPanel)]
    #[thread_kind = MainThreadOnly]
    struct QuickPanel;

    impl QuickPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool { true }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool { false }

        #[unsafe(method(resignKeyWindow))]
        fn resign_key(&self) {
            // SAFETY: Standard superclass focus handling must precede hiding.
            unsafe { let _: () = msg_send![super(self), resignKeyWindow]; }
            self.orderOut(None);
            mark_hidden();
        }
    }
);

struct Host {
    panel: Retained<QuickPanel>,
    work: NSRect,
    anchor: NSRect,
}
thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    static APP: RefCell<Option<tauri::AppHandle>> = const { RefCell::new(None) };
}

fn mark_hidden() {
    APP.with(|slot| {
        if let Some(app) = slot.borrow().as_ref() {
            if let Ok(mut time) = app.state::<super::PanelState>().hidden.lock() {
                *time = Some(std::time::Instant::now());
            }
            let _ = app.emit_to(super::LABEL, "tray-panel-active", false);
        }
    });
}

fn frame(work: NSRect, anchor: NSRect) -> NSRect {
    let width = 440_f64.min((work.size.width - 12.0).max(1.0));
    let height = super::HEIGHT.min((work.size.height - 16.0).max(1.0));
    let left = work.origin.x + 6.0;
    let right = (work.origin.x + work.size.width - width - 6.0).max(left);
    NSRect::new(
        NSPoint::new(
            (anchor.origin.x + (anchor.size.width - width) / 2.0).clamp(left, right),
            work.origin.y + work.size.height - height - 6.0,
        ),
        NSSize::new(width, height),
    )
}

fn anchor(app: &tauri::AppHandle, mtm: MainThreadMarker) -> Result<(NSRect, NSRect), String> {
    let tray = app
        .tray_by_id(crate::tray::TRAY_ID)
        .ok_or("trayPanel.failed")?;
    let found = tray
        .with_inner_tray_icon(|tray| {
            let mtm = MainThreadMarker::new()?;
            // The native status item owns the clicked menu-bar screen. Do not convert
            // its global origin using a different window's backing scale factor.
            let item = tray.ns_status_item()?;
            let button = item.button(mtm)?;
            let window = button.window()?;
            let screen = window.screen()?;
            let rect = window.convertRectToScreen(button.frame());
            let work = screen.visibleFrame();
            Some((
                [
                    rect.origin.x,
                    rect.origin.y,
                    rect.size.width,
                    rect.size.height,
                ],
                [
                    work.origin.x,
                    work.origin.y,
                    work.size.width,
                    work.size.height,
                ],
            ))
        })
        .map_err(|_| "trayPanel.failed")?;
    if let Some((rect, work)) = found {
        return Ok((
            NSRect::new(
                NSPoint::new(rect[0], rect[1]),
                NSSize::new(rect[2], rect[3]),
            ),
            NSRect::new(
                NSPoint::new(work[0], work[1]),
                NSSize::new(work[2], work[3]),
            ),
        ));
    }
    // Native status-item bounds may disappear while displays are being changed.
    // Prefer the cursor's screen, never silently the main window's screen.
    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|screen| {
            let f = screen.frame();
            mouse.x >= f.origin.x
                && mouse.x < f.origin.x + f.size.width
                && mouse.y >= f.origin.y
                && mouse.y < f.origin.y + f.size.height
        })
        .ok_or("trayPanel.failed")?;
    Ok((
        NSRect::new(mouse, NSSize::new(0.0, 0.0)),
        screen.visibleFrame(),
    ))
}

pub(super) fn show(app: &tauri::AppHandle) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("trayPanel.failed")?;
    let (anchor, work) = anchor(app, mtm)?;
    APP.with(|slot| *slot.borrow_mut() = Some(app.clone()));
    HOST.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let window = app.get_webview_window(super::LABEL).ok_or("trayPanel.failed")?;
            let ptr = window.ns_window().map_err(|_| "trayPanel.failed")?;
            // SAFETY: Tauri keeps its NSWindow alive, and this runs on its main
            // thread. Move the retained content view, not the window's class or
            // delegate: Tao's original lifecycle remains intact.
            let original = unsafe { &*ptr.cast::<NSWindow>() };
            let content = original.contentView().ok_or("trayPanel.failed")?;
            let panel: Retained<QuickPanel> = unsafe {
                msg_send![QuickPanel::alloc(mtm), initWithContentRect: frame(work, anchor), styleMask: NSWindowStyleMask::NonactivatingPanel, backing: NSBackingStoreType::Buffered, defer: false]
            };
            panel.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary);
            panel.setLevel(101); // AppKit's popup-menu level.
            panel.setHidesOnDeactivate(false);
            panel.setFloatingPanel(true);
            panel.setBecomesKeyOnlyIfNeeded(false);
            // Keep the webview transparent above a system blur of the windows behind it.
            panel.setOpaque(false);
            panel.setBackgroundColor(Some(&NSColor::clearColor()));
            let surface = NSView::initWithFrame(NSView::alloc(mtm), content.bounds());
            let glass = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), surface.bounds());
            glass.setMaterial(NSVisualEffectMaterial::Popover);
            glass.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            glass.setState(NSVisualEffectState::Active);
            glass.setEmphasized(false);
            surface.setWantsLayer(true);
            let layer = surface.layer().ok_or("trayPanel.failed")?;
            // SAFETY: This retained CALayer is main-thread owned; CGFloat is f64 on macOS.
            unsafe { let _: () = msg_send![&*layer, setCornerRadius: 12.0_f64]; }
            layer.setMasksToBounds(true);
            panel.setHasShadow(true);
            unsafe { panel.setReleasedWhenClosed(false); }
            // SAFETY: Tauri retains this host window. AppKit observes its theme changes.
            unsafe { panel.setAppearanceSource(Some(original)); }
            original.setContentView(None);
            let sizing = NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable;
            content.setFrame(surface.bounds());
            content.setAutoresizingMask(sizing);
            glass.setAutoresizingMask(sizing);
            // Keep the glass and WebView as siblings so material appearance never themes the content.
            surface.addSubview(&glass);
            surface.addSubview(&content);
            panel.setContentView(Some(&surface));
            *slot = Some(Host { panel, work, anchor });
        }
        let host = slot.as_mut().ok_or("trayPanel.failed")?;
        host.work = work;
        host.anchor = anchor;
        host.panel.setFrame_display(frame(work, anchor), true);
        // Non-activating panels take the keyboard without activating the app or
        // changing to the Space occupied by its unrelated main window.
        host.panel.orderFrontRegardless();
        host.panel.makeKeyWindow();
        Ok::<_, String>(())
    })?;
    let _ = app.emit_to(super::LABEL, "tray-panel-refresh", ());
    let _ = app.emit_to(super::LABEL, "tray-panel-active", true);
    Ok(())
}

pub(super) fn hide_if_open() -> Result<bool, String> {
    MainThreadMarker::new().ok_or("trayPanel.failed")?;
    let hidden = HOST.with(|slot| {
        let slot = slot.borrow();
        let Some(host) = slot.as_ref().filter(|host| host.panel.isVisible()) else {
            return false;
        };
        host.panel.orderOut(None);
        true
    });
    if hidden {
        mark_hidden();
    }
    Ok(hidden)
}

pub(super) fn resize() -> Result<(), String> {
    MainThreadMarker::new().ok_or("trayPanel.failed")?;
    HOST.with(|slot| {
        if let Some(host) = slot.borrow().as_ref() {
            let old = host.panel.frame();
            let mut next = frame(host.work, host.anchor);
            next.origin.x = old.origin.x;
            next.origin.y = old.origin.y + old.size.height - next.size.height;
            if (next.size.height - old.size.height).abs() > 0.5 {
                host.panel
                    .setFrame_display_animate(next, true, host.panel.isVisible());
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_secondary_screen_points_without_primary_dpi_conversion() {
        let work = NSRect::new(NSPoint::new(-1920.0, -200.0), NSSize::new(1920.0, 1056.0));
        let anchor = NSRect::new(NSPoint::new(-180.0, 856.0), NSSize::new(22.0, 24.0));
        let panel = frame(work, anchor);
        assert_eq!(panel.size.width, 440.0);
        assert_eq!(panel.size.height, super::super::HEIGHT);
        assert_eq!(
            panel.origin.y + panel.size.height,
            work.origin.y + work.size.height - 6.0
        );
        assert!(panel.origin.x >= work.origin.x && panel.origin.x + panel.size.width <= 0.0);
        assert!(panel.origin.y >= work.origin.y);
        let short_work = NSRect::new(work.origin, NSSize::new(360.0, 400.0));
        let short = frame(short_work, anchor);
        assert_eq!(short.size.height, 384.0);
        assert!(short.origin.y >= short_work.origin.y);
    }
}
