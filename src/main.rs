use std::cell::Cell;
use std::rc::Rc;

use i_slint_backend_winit::winit::event::{ElementState, MouseButton, WindowEvent};
use i_slint_backend_winit::{EventResult, WinitWindowAccessor};
use slint::ComponentHandle;

slint::include_modules!();

const PANEL_WIDTH: f64 = 750.0;
const PANEL_HEIGHT: f64 = 474.0;
const DRAG_AREA_HEIGHT: f64 = 64.0;
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
    let app = AppWindow::new()?;

    configure_overlay_window(&app);
    install_panel_drag(&app);

    app.show()?;
    slint::run_event_loop()
}

fn configure_overlay_window(app: &AppWindow) {
    let slint_window = app.window();

    if let Some(geometry) = platform_screen_geometry() {
        app.set_screen_width(geometry.logical_size.width);
        app.set_screen_height(geometry.logical_size.height);
        app.set_panel_x(((geometry.logical_size.width as f64 - PANEL_WIDTH) / 2.0).max(0.0) as f32);
        app.set_panel_y(
            ((geometry.logical_size.height as f64 - PANEL_HEIGHT) / 2.0).max(0.0) as f32,
        );

        slint_window.set_position(geometry.position);
        slint_window.set_size(geometry.physical_size);
    } else {
        slint_window.set_fullscreen(true);
    }
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

fn cursor_is_in_drag_area(app: &AppWindow, drag: &DragState) -> bool {
    let cursor_x = drag.cursor_x.get();
    let cursor_y = drag.cursor_y.get();
    let panel_x = app.get_panel_x() as f64;
    let panel_y = app.get_panel_y() as f64;

    cursor_x.is_finite()
        && cursor_y.is_finite()
        && cursor_x >= panel_x
        && cursor_x <= panel_x + PANEL_WIDTH
        && cursor_y >= panel_y
        && cursor_y <= panel_y + DRAG_AREA_HEIGHT
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
