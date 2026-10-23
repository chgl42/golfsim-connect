//! The app's windows through AppKit: overlay frames and window levels, ordering above GSPro's
//! window, full-screen spaces, the main window's size and aspect ratio (ADR-005).

use objc2::MainThreadMarker;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSApplication, NSNormalWindowLevel, NSScreenSaverWindowLevel, NSWindowCollectionBehavior,
    NSWindowOrderingMode, NSWorkspace, NSWorkspaceDidActivateApplicationNotification,
};
use objc2_core_foundation::CFArray;
use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
use std::ptr::NonNull;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use block2::RcBlock;
use objc2_app_kit::{NSWindow, NSWindowDidEndLiveResizeNotification, NSWindowWillStartLiveResizeNotification};
use objc2_foundation::{NSArray, NSDictionary, NSNotification, NSNotificationCenter, NSNumber, NSSize, NSString, NSTimer};

/// Aspect ratio the main window keeps while resizing (`None`: none, e.g. in full screen).
static ASPECT: Mutex<Option<(f64, f64)>> = Mutex::new(None);

/// Middle of GSPro's window when last seen on screen (window-server coordinates: points from
/// the top left of the first screen).
static GSPRO_AT: Mutex<Option<(f64, f64)>> = Mutex::new(None);

/// The screen the overlays are on: GSPro's (e.g. a projector), else the main window's.
fn overlay_screen(mtm: MainThreadMarker) -> Option<objc2::rc::Retained<objc2_app_kit::NSScreen>> {
    use objc2_app_kit::NSScreen;
    let screens = NSScreen::screens(mtm);
    let gspro_at = *GSPRO_AT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(i) = gspro_at.and_then(|at| screen_of(mtm, at)) {
        return Some(screens.objectAtIndex(i));
    }
    let main_title = NSString::from_str(super::APP_NAME);
    let windows = NSApplication::sharedApplication(mtm).windows();
    windows.iter().find(|w| w.title().isEqualToString(&main_title)).and_then(|w| w.screen()).or_else(|| NSScreen::mainScreen(mtm))
}

/// Put the content of the window titled `title` at `top_left` (points from the main screen's
/// top left corner) with `size` (points), if it isn't there already.
pub fn place_window(title: &str, top_left: eframe::egui::Pos2, size: eframe::egui::Vec2) {
    use objc2_foundation::{NSPoint, NSRect};
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(screen) = overlay_screen(mtm) else { return };
    let full = screen.frame();
    let title = NSString::from_str(title);
    let windows = NSApplication::sharedApplication(mtm).windows();
    let Some(window) = windows.iter().find(|w| w.title().isEqualToString(&title)) else { return };
    // AppKit counts from the bottom left.
    let content = NSRect::new(
        NSPoint::new(full.origin.x + f64::from(top_left.x), full.origin.y + full.size.height - f64::from(top_left.y) - f64::from(size.y)),
        NSSize::new(size.x.into(), size.y.into()),
    );
    let frame = window.frameRectForContentRect(content);
    let now = window.frame();
    let off = (now.origin.x - frame.origin.x).abs()
        + (now.origin.y - frame.origin.y).abs()
        + (now.size.width - frame.size.width).abs()
        + (now.size.height - frame.size.height).abs();
    if off > 0.5 {
        window.setFrame_display(frame, true);
    }
}

/// The mouse on the overlays' screen, in points from its top left corner.
pub fn mouse_location() -> Option<eframe::egui::Pos2> {
    use objc2_app_kit::NSEvent;
    let mtm = MainThreadMarker::new()?;
    let full = overlay_screen(mtm)?.frame();
    let at = NSEvent::mouseLocation();
    // AppKit counts from the bottom left.
    Some(eframe::egui::pos2((at.x - full.origin.x) as f32, (full.origin.y + full.size.height - at.y) as f32))
}

/// The overlays' screen without the menu bar and the Dock, in points from its top left corner.
pub fn visible_frame() -> Option<eframe::egui::Rect> {
    let mtm = MainThreadMarker::new()?;
    let screen = overlay_screen(mtm)?;
    let (full, visible) = (screen.frame(), screen.visibleFrame());
    // AppKit counts from the bottom left.
    let top = (full.origin.y + full.size.height) - (visible.origin.y + visible.size.height);
    let left = visible.origin.x - full.origin.x;
    Some(eframe::egui::Rect::from_min_size(
        eframe::egui::pos2(left as f32, top as f32),
        eframe::egui::vec2(visible.size.width as f32, visible.size.height as f32),
    ))
}

/// Largest content size the window titled `title` can have on its screen: the visible frame
/// (without menu bar and Dock) minus the title bar, in points. `None` before the window exists.
pub fn max_content_size(title: &str) -> Option<eframe::egui::Vec2> {
    let mtm = MainThreadMarker::new()?;
    let title = NSString::from_str(title);
    let windows = NSApplication::sharedApplication(mtm).windows();
    let window = windows.iter().find(|w| w.title().isEqualToString(&title))?;
    let visible = window.screen()?.visibleFrame();
    let frame = window.frame();
    let title_bar = frame.size.height - window.contentRectForFrameRect(frame).size.height;
    Some(eframe::egui::vec2(visible.size.width as f32, (visible.size.height - title_bar) as f32))
}

/// Set the content size and minimum content size (points) of the window titled `title`,
/// keeping its top edge and the window on the screen. False before the window exists.
pub fn set_content_size(title: &str, size: eframe::egui::Vec2, min: eframe::egui::Vec2) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let title = NSString::from_str(title);
    let windows = NSApplication::sharedApplication(mtm).windows();
    let Some(window) = windows.iter().find(|w| w.title().isEqualToString(&title)) else {
        return false;
    };
    window.setContentMinSize(NSSize::new(min.x.into(), min.y.into()));
    let frame = window.frame();
    let mut content = window.contentRectForFrameRect(frame);
    content.size = NSSize::new(size.x.into(), size.y.into());
    let mut new = window.frameRectForContentRect(content);
    // AppKit's origin is bottom-left: keep the top edge where it was.
    new.origin.y = frame.origin.y + frame.size.height - new.size.height;
    if let Some(screen) = window.screen() {
        let v = screen.visibleFrame();
        new.origin.x = new.origin.x.min(v.origin.x + v.size.width - new.size.width).max(v.origin.x);
        new.origin.y = new.origin.y.max(v.origin.y).min(v.origin.y + v.size.height - new.size.height);
    }
    window.setFrame_display(new, true);
    true
}

/// Let the window be resized only in the given aspect ratio (`None`: freely). macOS applies
/// it during the drag itself, which is smooth; correcting the size afterwards makes the
/// window jump. Returns false while the window doesn't exist yet.
pub fn lock_aspect_ratio(title: &str, ratio: Option<eframe::egui::Vec2>) -> bool {
    static OBSERVING: AtomicBool = AtomicBool::new(false);
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let title = NSString::from_str(title);
    let windows = NSApplication::sharedApplication(mtm).windows();
    let Some(window) = windows.iter().find(|w| w.title().isEqualToString(&title)) else {
        return false;
    };
    *ASPECT.lock().unwrap() = ratio.map(|r| (r.x.into(), r.y.into()));
    apply_aspect(&window);
    // winit resets the content resize increments when a live resize starts and ends, which
    // clears the aspect ratio; set it again right after (observers run in the order added).
    if !OBSERVING.swap(true, Ordering::Relaxed) {
        let center = NSNotificationCenter::defaultCenter();
        for name in unsafe { [NSWindowWillStartLiveResizeNotification, NSWindowDidEndLiveResizeNotification] } {
            let w = window.clone();
            let block = RcBlock::new(move |_: NonNull<NSNotification>| apply_aspect(&w));
            let token = unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), Some(&window), None, &block) };
            // Observes for the lifetime of the app.
            std::mem::forget(token);
        }
    }
    true
}

fn apply_aspect(window: &NSWindow) {
    match *ASPECT.lock().unwrap() {
        Some((w, h)) => window.setContentAspectRatio(NSSize::new(w, h)),
        // Setting resize increments is how AppKit clears an aspect ratio.
        None => window.setContentResizeIncrements(NSSize::new(1.0, 1.0)),
    }
}

/// Keep the window above full-screen apps: show it on every Space, including the
/// separate Space of a full-screen app, and place it for the app in front (see
/// [`arrange_overlays`]). `moving`: an overlay is being moved, so they all stay on top.
pub fn float_over_fullscreen(ctx: &eframe::egui::Context, title: &str, moving: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    CONTEXT.get_or_init(|| ctx.clone());
    let title = NSString::from_str(title);
    let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::FullScreenAuxiliary
        | NSWindowCollectionBehavior::Stationary
        | NSWindowCollectionBehavior::IgnoresCycle;
    for window in NSApplication::sharedApplication(mtm).windows() {
        if window.title().isEqualToString(&title) && window.collectionBehavior() != behavior {
            window.setCollectionBehavior(behavior);
        }
    }
    MOVING.store(moving, Ordering::Relaxed);
    if !WATCHING_APPS.swap(true, Ordering::Relaxed) {
        // Re-arranged when another app comes to the front, also while this one doesn't repaint.
        let workspace = NSWorkspace::sharedWorkspace();
        let block = RcBlock::new(|_: NonNull<NSNotification>| arrange_overlays(true));
        let token = unsafe {
            workspace.notificationCenter().addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceDidActivateApplicationNotification),
                None,
                None,
                &block,
            )
        };
        // Observes for the lifetime of the app.
        std::mem::forget(token);
        // And checked four times a second: right after a switch the window list may not show
        // the new order yet. The tick also keeps the overlays painted: egui stops painting
        // while all its windows are covered (the main window by GSPro, the overlays by the
        // main window) and doesn't start again when one of them shows.
        let tick = RcBlock::new(|_: NonNull<NSTimer>| {
            arrange_overlays(false);
            if let Some(ctx) = CONTEXT.get() {
                ctx.request_repaint();
            }
        });
        let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.25, true, &tick) };
        std::mem::forget(timer);
    }
    arrange_overlays(false);
}

/// An overlay is being moved.
static MOVING: AtomicBool = AtomicBool::new(false);
/// To repaint the overlays when they show again: a window that was hidden (or covered) while
/// the app didn't repaint comes back empty.
static CONTEXT: std::sync::OnceLock<eframe::egui::Context> = std::sync::OnceLock::new();
static WATCHING_APPS: AtomicBool = AtomicBool::new(false);
/// The app in front when the overlays were last arranged.
static ARRANGED_FOR: Mutex<Option<Front>> = Mutex::new(None);

/// The app in front, as far as the overlays care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Front {
    Ours,
    Gspro,
    Other,
}

/// GSPro's processes under CrossOver: "GSPro.exe", and "GSPconnect.exe" for its Connect window.
fn is_gspro(name: &str) -> bool {
    name.to_lowercase().starts_with("gsp")
}

/// A normal window of another app on screen.
struct OtherWindow {
    owner: String,
    number: isize,
    /// Its middle in window-server coordinates.
    middle: Option<(f64, f64)>,
}

/// The normal windows on screen of other apps, front to back.
fn other_windows() -> Vec<OtherWindow> {
    let options = CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(list) = CGWindowListCopyWindowInfo(options, 0) else { return Vec::new() };
    // A CFArray of CFDictionaries; toll-free bridged to Foundation.
    let list: &NSArray<NSDictionary<NSString, AnyObject>> = unsafe { &*std::ptr::from_ref::<CFArray>(&list).cast() };
    let get = |info: &NSDictionary<NSString, AnyObject>, key: &str| info.objectForKey(&NSString::from_str(key));
    let number = |info: &NSDictionary<NSString, AnyObject>, key: &str| {
        get(info, key).and_then(|o| o.downcast::<NSNumber>().ok()).map(|n| n.integerValue())
    };
    let own = isize::try_from(std::process::id()).unwrap_or(-1);
    list.iter()
        .filter(|info| number(info, "kCGWindowLayer") == Some(0) && number(info, "kCGWindowOwnerPID") != Some(own))
        .filter_map(|info| {
            let owner = get(&info, "kCGWindowOwnerName").and_then(|o| o.downcast::<NSString>().ok())?.to_string();
            let bounds = get(&info, "kCGWindowBounds").and_then(|o| o.downcast::<NSDictionary>().ok());
            let middle = bounds.and_then(|b| {
                let v = |k: &str| b.objectForKey(&NSString::from_str(k)).and_then(|o| o.downcast::<NSNumber>().ok()).map(|n| n.doubleValue());
                Some((v("X")? + v("Width")? / 2.0, v("Y")? + v("Height")? / 2.0))
            });
            Some(OtherWindow { owner, number: number(&info, "kCGWindowNumber")?, middle })
        })
        .collect()
}

/// The screen (its index) a point in window-server coordinates is on.
fn screen_of(mtm: MainThreadMarker, (x, y): (f64, f64)) -> Option<usize> {
    let screens = objc2_app_kit::NSScreen::screens(mtm);
    // AppKit counts from the bottom left of the first screen.
    let y = screens.firstObject()?.frame().size.height - y;
    screens.iter().position(|s| {
        let f = s.frame();
        x >= f.origin.x && x < f.origin.x + f.size.width && y >= f.origin.y && y < f.origin.y + f.size.height
    })
}

/// Who is in front on GSPro's screen: GSPro if it is the active app (the name macOS gives it
/// under CrossOver isn't reliable, so also if its window is the top one there), else this app
/// if it is active and its window is on that screen, else another app.
fn front(mtm: MainThreadMarker) -> Front {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication();
    let ours = app.as_ref().is_some_and(|a| i64::from(a.processIdentifier()) == i64::from(std::process::id()));
    let named_gspro = app.and_then(|a| a.localizedName()).is_some_and(|n| is_gspro(&n.to_string()));
    let windows = other_windows();
    // Remember where GSPro is: the overlays go to its screen.
    if let Some(at) = windows.iter().find(|w| is_gspro(&w.owner)).and_then(|w| w.middle) {
        *GSPRO_AT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(at);
    }
    let gspro_at = *GSPRO_AT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let gspro_screen = gspro_at.and_then(|at| screen_of(mtm, at));
    if ours {
        // This app's window covers GSPro only on GSPro's screen.
        let main_title = NSString::from_str(super::APP_NAME);
        let windows = NSApplication::sharedApplication(mtm).windows();
        let main = windows.iter().find(|w| w.title().isEqualToString(&main_title)).map(|w| w.frame());
        let main_screen = main.and_then(|f| {
            let first = objc2_app_kit::NSScreen::screens(mtm).firstObject()?.frame();
            // Its middle in window-server coordinates (from the top left of the first screen).
            let middle = (f.origin.x + f.size.width / 2.0, first.size.height - (f.origin.y + f.size.height / 2.0));
            screen_of(mtm, middle)
        });
        if gspro_screen.is_none() || main_screen == gspro_screen {
            return Front::Ours;
        }
    } else if named_gspro {
        return Front::Gspro;
    }
    // The top window on GSPro's screen (of another app) decides.
    let top_there = windows.iter().find(|w| gspro_screen.is_none() || w.middle.and_then(|m| screen_of(mtm, m)) == gspro_screen);
    match top_there {
        Some(w) if is_gspro(&w.owner) => Front::Gspro,
        _ if ours => Front::Ours,
        _ => Front::Other,
    }
}

/// GSPro's frontmost window on screen (its window-server number), if GSPro runs.
fn gspro_window() -> Option<isize> {
    other_windows().into_iter().find(|w| is_gspro(&w.owner)).map(|w| w.number)
}

/// The overlays sit right above GSPro: on top of everything while GSPro is in front (also in
/// full screen), else right above GSPro's window, under any window in front of it, this app's
/// too (hidden when GSPro doesn't run). `changed`: another app came to the front.
fn arrange_overlays(changed: bool) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let front = if MOVING.load(Ordering::Relaxed) { Front::Gspro } else { front(mtm) };
    let mut last = ARRANGED_FOR.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let reorder = changed || *last != Some(front);
    *last = Some(front);
    drop(last);
    let app = NSApplication::sharedApplication(mtm);
    let windows = app.windows();
    let overlays: Vec<_> = windows
        .iter()
        .filter(|w| {
            let t = w.title().to_string();
            t.starts_with(super::OVERLAY_TITLE) || t.starts_with(super::TILE_OVERLAY_TITLE)
        })
        .collect();
    if overlays.is_empty() {
        return;
    }
    // Where they go: their level, whether they show, and the window they go behind or above.
    let (level, shown, place) = match front {
        Front::Gspro => (NSScreenSaverWindowLevel, true, None),
        Front::Ours | Front::Other => match gspro_window().filter(|_| reorder) {
            Some(n) => (NSNormalWindowLevel, true, Some((NSWindowOrderingMode::Above, n))),
            None if reorder => (NSNormalWindowLevel, false, None),
            // Placed when that app came to the front; only new overlays still need it.
            None => (NSNormalWindowLevel, overlays.iter().any(|w| w.alphaValue() > 0.0), None),
        },
    };
    let alpha = if shown { 1.0 } else { 0.0 };
    let mut moved = false;
    for window in overlays {
        let new = window.level() != level || (window.alphaValue() - alpha).abs() > f64::EPSILON;
        if !new && !reorder {
            continue;
        }
        window.setLevel(level);
        window.setAlphaValue(alpha);
        if let Some((mode, other)) = place {
            window.orderWindow_relativeTo(mode, other);
        } else if front == Front::Gspro {
            // A higher level alone doesn't bring it back into GSPro's full-screen Space.
            window.orderFrontRegardless();
        }
        moved = true;
    }
    if moved && let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}
