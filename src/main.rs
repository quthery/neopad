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

#[cfg(target_os = "windows")]
fn apply_platform_window_effects(window: &i_slint_backend_winit::winit::window::Window) {
    use i_slint_backend_winit::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Dwm::{
        DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DwmSetWindowAttribute,
    };

    let Ok(handle) = window.window_handle() else {
        return;
    };

    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };

    let hwnd = handle.hwnd.get() as isize as windows_sys::Win32::Foundation::HWND;
    let backdrop = DWMSBT_TRANSIENTWINDOW;

    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            &backdrop as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&backdrop) as u32,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_platform_window_effects(_window: &i_slint_backend_winit::winit::window::Window) {}
