use std::cell::Cell;
use std::rc::Rc;

use i_slint_backend_winit::winit::event::{ElementState, MouseButton, WindowEvent};
use i_slint_backend_winit::winit::keyboard::{Key, NamedKey};
use i_slint_backend_winit::{EventResult, WinitWindowAccessor};
use slint::ComponentHandle;

slint::include_modules!();

const PANEL_WIDTH: f64 = 774.0;
const PANEL_HEIGHT: f64 = 340.0;
const DRAG_AREA_HEIGHT: f64 = 60.0;
const SEARCH_FIELD_X: f64 = 16.0;
const SEARCH_FIELD_WIDTH: f64 = 540.0;
const SEARCH_FIELD_HEIGHT: f64 = 40.0;
const SEARCH_FIELD_Y: f64 = (DRAG_AREA_HEIGHT - SEARCH_FIELD_HEIGHT) / 2.0;
const SNAP_THRESHOLD: f64 = 18.0;

struct ScreenGeometry {
    position: slint::PhysicalPosition,
    physical_size: slint::PhysicalSize,
    logical_size: slint::LogicalSize,
}

#[derive(Default)]
struct DragState {
    active: Cell<bool>,
    cursor_x: Cell<f64>,
    cursor_y: Cell<f64>,
    start_cursor_x: Cell<f64>,
    start_cursor_y: Cell<f64>,
    start_panel_x: Cell<f64>,
    start_panel_y: Cell<f64>,
}

fn main() -> Result<(), slint::PlatformError> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("skia".into())
        .select()?;

    let app = AppWindow::new()?;
    app.set_drag_area_height(DRAG_AREA_HEIGHT as f32);

    configure_initial_overlay_window(&app);
    configure_window_effects(&app.window());
    install_panel_drag(&app);

    app.show()?;
    configure_overlay_window(&app);
    slint::run_event_loop()
}

fn configure_initial_overlay_window(app: &AppWindow) {
    if let Some(geometry) = platform_screen_geometry() {
        apply_screen_geometry(app, &geometry);
    }
}

fn configure_overlay_window(app: &AppWindow) {
    let slint_window = app.window();
    configure_window_effects(&slint_window);

    if let Some(geometry) =
        platform_screen_geometry().or_else(|| winit_screen_geometry(&slint_window))
    {
        apply_screen_geometry(app, &geometry);
    } else {
        slint_window.set_fullscreen(true);
    }
}

fn apply_screen_geometry(app: &AppWindow, geometry: &ScreenGeometry) {
    let slint_window = app.window();

    app.set_screen_width(geometry.logical_size.width);
    app.set_screen_height(geometry.logical_size.height);
    app.set_panel_x(((geometry.logical_size.width as f64 - PANEL_WIDTH) / 2.0).max(0.0) as f32);
    app.set_panel_y(((geometry.logical_size.height as f64 - PANEL_HEIGHT) / 2.0).max(0.0) as f32);
    slint_window.set_position(geometry.position);
    slint_window.set_size(geometry.physical_size);
}

fn winit_screen_geometry(slint_window: &slint::Window) -> Option<ScreenGeometry> {
    slint_window
        .with_winit_window(|window| {
            let monitor = window.current_monitor()?;
            let position = monitor.position();
            let physical_size = monitor.size();
            let scale_factor = monitor.scale_factor();

            Some(ScreenGeometry {
                position: slint::PhysicalPosition::new(position.x, position.y),
                physical_size: slint::PhysicalSize::new(physical_size.width, physical_size.height),
                logical_size: slint::LogicalSize::new(
                    (physical_size.width as f64 / scale_factor) as f32,
                    (physical_size.height as f64 / scale_factor) as f32,
                ),
            })
        })
        .flatten()
}

fn configure_window_effects(slint_window: &slint::Window) {
    slint_window.with_winit_window(|window| {
        window.set_transparent(true);
        apply_platform_window_effects(window);
    });
}

fn install_panel_drag(app: &AppWindow) {
    let app_weak = app.as_weak();
    let drag = Rc::new(DragState {
        cursor_x: Cell::new(f64::INFINITY),
        cursor_y: Cell::new(f64::INFINITY),
        ..Default::default()
    });

    app.window().on_winit_window_event({
        let drag = drag.clone();

        move |slint_window, event| {
            let Some(app) = app_weak.upgrade() else {
                return EventResult::Propagate;
            };

            match event {
                WindowEvent::CursorMoved { position, .. } => {
                    let scale_factor = slint_window.scale_factor() as f64;
                    let cursor_x = position.x / scale_factor;
                    let cursor_y = position.y / scale_factor;

                    drag.cursor_x.set(cursor_x);
                    drag.cursor_y.set(cursor_y);

                    if drag.active.get() {
                        let panel_x =
                            drag.start_panel_x.get() + cursor_x - drag.start_cursor_x.get();
                        let panel_y =
                            drag.start_panel_y.get() + cursor_y - drag.start_cursor_y.get();

                        set_panel_position(&app, slint_window, panel_x, panel_y);
                        return EventResult::PreventDefault;
                    }
                }
                WindowEvent::CursorLeft { .. } if !drag.active.get() => {
                    drag.cursor_x.set(f64::INFINITY);
                    drag.cursor_y.set(f64::INFINITY);
                }
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed
                        && matches!(event.logical_key.as_ref(), Key::Named(NamedKey::Escape)) =>
                {
                    quit_application();
                    return EventResult::PreventDefault;
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    ..
                } if cursor_is_outside_panel(&app, &drag) => {
                    quit_application();
                    return EventResult::PreventDefault;
                }
                WindowEvent::MouseInput {
                    button: MouseButton::Left,
                    state: ElementState::Pressed,
                    ..
                } if cursor_is_in_drag_area(&app, &drag) => {
                    drag.active.set(true);
                    drag.start_cursor_x.set(drag.cursor_x.get());
                    drag.start_cursor_y.set(drag.cursor_y.get());
                    drag.start_panel_x.set(app.get_panel_x() as f64);
                    drag.start_panel_y.set(app.get_panel_y() as f64);

                    app.set_dragging(true);
                    set_panel_position(
                        &app,
                        slint_window,
                        drag.start_panel_x.get(),
                        drag.start_panel_y.get(),
                    );

                    return EventResult::PreventDefault;
                }
                WindowEvent::MouseInput {
                    button: MouseButton::Left,
                    state: ElementState::Released,
                    ..
                } if drag.active.get() => {
                    drag.active.set(false);
                    app.set_dragging(false);
                    app.set_panel_centered_x(false);
                    app.set_panel_centered_y(false);
                    return EventResult::PreventDefault;
                }
                _ => {}
            }

            EventResult::Propagate
        }
    });
}

fn quit_application() {
    if let Err(error) = slint::quit_event_loop() {
        eprintln!("failed to quit event loop: {error}");
    }
}

fn cursor_is_in_drag_area(app: &AppWindow, drag: &DragState) -> bool {
    let cursor_x = drag.cursor_x.get();
    let cursor_y = drag.cursor_y.get();
    let panel_x = app.get_panel_x() as f64;
    let panel_y = app.get_panel_y() as f64;

    let in_search_field = cursor_x >= panel_x + SEARCH_FIELD_X
        && cursor_x <= panel_x + SEARCH_FIELD_X + SEARCH_FIELD_WIDTH
        && cursor_y >= panel_y + SEARCH_FIELD_Y
        && cursor_y <= panel_y + SEARCH_FIELD_Y + SEARCH_FIELD_HEIGHT;

    cursor_x.is_finite()
        && cursor_y.is_finite()
        && !in_search_field
        && cursor_x >= panel_x
        && cursor_x <= panel_x + PANEL_WIDTH
        && cursor_y >= panel_y
        && cursor_y <= panel_y + DRAG_AREA_HEIGHT
}

fn cursor_is_outside_panel(app: &AppWindow, drag: &DragState) -> bool {
    let cursor_x = drag.cursor_x.get();
    let cursor_y = drag.cursor_y.get();

    cursor_x.is_finite() && cursor_y.is_finite() && !cursor_is_in_panel(app, cursor_x, cursor_y)
}

fn cursor_is_in_panel(app: &AppWindow, cursor_x: f64, cursor_y: f64) -> bool {
    let panel_x = app.get_panel_x() as f64;
    let panel_y = app.get_panel_y() as f64;

    cursor_x >= panel_x
        && cursor_x <= panel_x + PANEL_WIDTH
        && cursor_y >= panel_y
        && cursor_y <= panel_y + PANEL_HEIGHT
}

fn set_panel_position(app: &AppWindow, _slint_window: &slint::Window, panel_x: f64, panel_y: f64) {
    let max_x = (app.get_screen_width() as f64 - PANEL_WIDTH).max(0.0);
    let max_y = (app.get_screen_height() as f64 - PANEL_HEIGHT).max(0.0);

    let mut x = panel_x.clamp(0.0, max_x);
    let mut y = panel_y.clamp(0.0, max_y);

    let center_x = max_x / 2.0;
    let center_y = max_y / 2.0;
    let centered_x = (x - center_x).abs() <= SNAP_THRESHOLD;
    let centered_y = centered_x && (y - center_y).abs() <= SNAP_THRESHOLD;

    if centered_x {
        x = center_x;
    }

    if centered_y {
        y = center_y;
    }

    app.set_panel_x(x as f32);
    app.set_panel_y(y as f32);
    app.set_panel_centered_x(centered_x);
    app.set_panel_centered_y(centered_y);
}

#[cfg(target_os = "macos")]
fn platform_screen_geometry() -> Option<ScreenGeometry> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    let main_thread = MainThreadMarker::new()?;
    let screen = NSScreen::mainScreen(main_thread)?;
    let frame = screen.frame();
    let backing_frame = screen.convertRectToBacking(frame);

    Some(ScreenGeometry {
        position: slint::PhysicalPosition::new(
            backing_frame.origin.x.round() as i32,
            backing_frame.origin.y.round() as i32,
        ),
        physical_size: slint::PhysicalSize::new(
            backing_frame.size.width.round() as u32,
            backing_frame.size.height.round() as u32,
        ),
        logical_size: slint::LogicalSize::new(frame.size.width as f32, frame.size.height as f32),
    })
}

#[cfg(not(target_os = "macos"))]
fn platform_screen_geometry() -> Option<ScreenGeometry> {
    None
}

#[cfg(target_os = "macos")]
fn apply_platform_window_effects(window: &i_slint_backend_winit::winit::window::Window) {
    use i_slint_backend_winit::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use objc2::{ClassType, MainThreadMarker};
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameVibrantDark,
        NSAutoresizingMaskOptions, NSColor, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    };

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };

    let Ok(handle) = window.window_handle() else {
        return;
    };

    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };

    let slint_view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let Some(ns_window) = slint_view.window() else {
        return;
    };
    let Some(content_view) = ns_window.contentView() else {
        return;
    };

    ns_window.setOpaque(false);
    ns_window.setBackgroundColor(Some(&NSColor::clearColor()));

    if !std::ptr::eq(content_view.as_ref(), slint_view) {
        return;
    }

    let bounds = slint_view.bounds();
    let resize_mask =
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable;
    let effect_view = NSVisualEffectView::initWithFrame(main_thread.alloc(), bounds);
    effect_view.setMaterial(NSVisualEffectMaterial::Popover);
    effect_view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect_view.setState(NSVisualEffectState::Active);
    effect_view.setAutoresizingMask(resize_mask);

    if let Some(appearance) = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameVibrantDark })
    {
        effect_view.setAppearance(Some(&appearance));
    }

    slint_view.setFrame(bounds);
    slint_view.setAutoresizingMask(resize_mask);
    effect_view.addSubview(slint_view);
    ns_window.setContentView(Some(effect_view.as_super()));
}

#[cfg(target_os = "linux")]
fn apply_platform_window_effects(window: &i_slint_backend_winit::winit::window::Window) {
    use i_slint_backend_winit::winit::raw_window_handle::{
        HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
    };

    let Ok(window_handle) = window.window_handle() else {
        return;
    };
    let Ok(display_handle) = window.display_handle() else {
        return;
    };

    match (display_handle.as_raw(), window_handle.as_raw()) {
        (RawDisplayHandle::Xlib(display), RawWindowHandle::Xlib(window)) => {
            let Some(display) = display.display else {
                return;
            };
            unsafe {
                apply_xlib_blur(display.as_ptr(), window.window);
            }
        }
        (RawDisplayHandle::Xcb(display), RawWindowHandle::Xcb(window)) => {
            let Some(connection) = display.connection else {
                return;
            };
            unsafe {
                apply_xcb_blur(connection.as_ptr(), window.window.get());
            }
        }
        _ => {}
    }
}

#[cfg(target_os = "windows")]
fn apply_platform_window_effects(window: &i_slint_backend_winit::winit::window::Window) {
    use i_slint_backend_winit::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Dwm::{
        DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
    };
    use windows_sys::Win32::UI::Controls::MARGINS;

    let Ok(handle) = window.window_handle() else {
        return;
    };

    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };

    let hwnd = handle.hwnd.get() as isize as windows_sys::Win32::Foundation::HWND;
    let backdrop = DWMSBT_TRANSIENTWINDOW;
    let dark_mode = 1_i32;
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };

    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &dark_mode as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&dark_mode) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            &backdrop as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&backdrop) as u32,
        );
    }
}

#[cfg(all(
    not(target_os = "linux"),
    not(target_os = "macos"),
    not(target_os = "windows")
))]
fn apply_platform_window_effects(_window: &i_slint_backend_winit::winit::window::Window) {}

#[cfg(target_os = "linux")]
unsafe fn apply_xlib_blur(display: *mut core::ffi::c_void, window: core::ffi::c_ulong) {
    use core::ffi::{c_char, c_int, c_uchar, c_ulong};

    #[link(name = "X11")]
    unsafe extern "C" {
        fn XInternAtom(
            display: *mut core::ffi::c_void,
            atom_name: *const c_char,
            only_if_exists: c_int,
        ) -> c_ulong;
        fn XChangeProperty(
            display: *mut core::ffi::c_void,
            window: c_ulong,
            property: c_ulong,
            property_type: c_ulong,
            format: c_int,
            mode: c_int,
            data: *const c_uchar,
            nelements: c_int,
        ) -> c_int;
        fn XFlush(display: *mut core::ffi::c_void) -> c_int;
    }

    const PROP_MODE_REPLACE: c_int = 0;

    let property = unsafe { XInternAtom(display, c"_KDE_NET_WM_BLUR_BEHIND_REGION".as_ptr(), 0) };
    let cardinal = unsafe { XInternAtom(display, c"CARDINAL".as_ptr(), 0) };

    if property == 0 || cardinal == 0 {
        return;
    }

    unsafe {
        XChangeProperty(
            display,
            window,
            property,
            cardinal,
            32,
            PROP_MODE_REPLACE,
            core::ptr::null(),
            0,
        );
        XFlush(display);
    }
}

#[cfg(target_os = "linux")]
unsafe fn apply_xcb_blur(connection: *mut core::ffi::c_void, window: u32) {
    use core::ffi::{c_char, c_int, c_void};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct XcbInternAtomCookie {
        sequence: u32,
    }

    #[repr(C)]
    struct XcbInternAtomReply {
        response_type: u8,
        pad0: u8,
        sequence: u16,
        length: u32,
        atom: u32,
    }

    #[link(name = "xcb")]
    unsafe extern "C" {
        fn xcb_intern_atom(
            connection: *mut c_void,
            only_if_exists: u8,
            name_len: u16,
            name: *const c_char,
        ) -> XcbInternAtomCookie;
        fn xcb_intern_atom_reply(
            connection: *mut c_void,
            cookie: XcbInternAtomCookie,
            error: *mut *mut c_void,
        ) -> *mut XcbInternAtomReply;
        fn xcb_change_property(
            connection: *mut c_void,
            mode: u8,
            window: u32,
            property: u32,
            property_type: u32,
            format: u8,
            data_len: u32,
            data: *const c_void,
        );
        fn xcb_flush(connection: *mut c_void) -> c_int;
    }

    unsafe extern "C" {
        fn free(ptr: *mut c_void);
    }

    const PROP_MODE_REPLACE: u8 = 0;

    unsafe fn intern_atom(connection: *mut c_void, name: &[u8]) -> u32 {
        let cookie = unsafe {
            xcb_intern_atom(
                connection,
                0,
                name.len() as u16,
                name.as_ptr() as *const c_char,
            )
        };
        let reply = unsafe { xcb_intern_atom_reply(connection, cookie, core::ptr::null_mut()) };

        if reply.is_null() {
            return 0;
        }

        let atom = unsafe { (*reply).atom };
        unsafe {
            free(reply as *mut c_void);
        }
        atom
    }

    let property = unsafe { intern_atom(connection, b"_KDE_NET_WM_BLUR_BEHIND_REGION") };
    let cardinal = unsafe { intern_atom(connection, b"CARDINAL") };

    if property == 0 || cardinal == 0 {
        return;
    }

    unsafe {
        xcb_change_property(
            connection,
            PROP_MODE_REPLACE,
            window,
            property,
            cardinal,
            32,
            0,
            core::ptr::null(),
        );
        xcb_flush(connection);
    }
}
