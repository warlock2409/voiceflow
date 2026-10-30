//! Window creation and floating-bar placement.

use tauri::{AppHandle, LogicalPosition, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const MAIN: &str = "main";
pub const BAR: &str = "bar";

/// Logical size of the bar window (the pill is centred inside with transparent margin).
const BAR_WIDTH: f64 = 380.0;
const BAR_HEIGHT: f64 = 56.0;
/// Gap between the bar and the bottom of the work area (i.e. the top of the taskbar).
const BAR_MARGIN: f64 = 12.0;

pub fn create_main<R: Runtime>(app: &AppHandle<R>, visible: bool) -> tauri::Result<WebviewWindow<R>> {
    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("VoiceFlow")
        .inner_size(560.0, 680.0)
        .min_inner_size(420.0, 480.0)
        .visible(visible)
        // Tauri defaults to focused=true, which made the *hidden* main window take the
        // foreground at launch, stealing focus from the user's app (and starving the
        // Caps Lock hook while the webview loaded). `show_main` focuses it explicitly.
        .focused(visible)
        .build()
}

/// A frameless, transparent, always-on-top, non-activating overlay. Being non-focusable
/// means clicking it or pressing the hotkey never steals focus from the user's app.
pub fn create_bar<R: Runtime>(app: &AppHandle<R>, visible: bool) -> tauri::Result<WebviewWindow<R>> {
    let bar = WebviewWindowBuilder::new(app, BAR, WebviewUrl::App("bar.html".into()))
        .title("VoiceFlow bar")
        .inner_size(BAR_WIDTH, BAR_HEIGHT)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()?;
    position_bar(&bar)?;
    if visible {
        super::show_no_activate(&bar)?;
    }
    Ok(bar)
}

/// Bottom-centre of the work area of the monitor the bar is on (falls back to primary).
/// Using the work area keeps the bar clear of the taskbar wherever it is docked.
pub fn position_bar<R: Runtime>(bar: &WebviewWindow<R>) -> tauri::Result<()> {
    let monitor = match bar.current_monitor()? {
        Some(m) => Some(m),
        None => bar.primary_monitor()?,
    };
    let Some(monitor) = monitor else { return Ok(()) };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let (x, y) = bar_origin(
        (area.position.x as f64 / scale, area.position.y as f64 / scale),
        (area.size.width as f64 / scale, area.size.height as f64 / scale),
    );
    bar.set_position(LogicalPosition::new(x, y))
}

/// Pure placement maths (logical px) so it can be unit-tested.
pub fn bar_origin(area_pos: (f64, f64), area_size: (f64, f64)) -> (f64, f64) {
    let x = area_pos.0 + ((area_size.0 - BAR_WIDTH) / 2.0).max(0.0);
    let y = area_pos.1 + (area_size.1 - BAR_HEIGHT - BAR_MARGIN).max(0.0);
    (x, y)
}

pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn set_bar_visible<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    if let Some(bar) = app.get_webview_window(BAR) {
        let _ = if visible {
            position_bar(&bar).and_then(|_| super::show_no_activate(&bar))
        } else {
            bar.hide()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_centre_above_bottom_taskbar() {
        // 1920x1080 with a 48 px bottom taskbar -> work area height 1032.
        let (x, y) = bar_origin((0.0, 0.0), (1920.0, 1032.0));
        assert_eq!(x, (1920.0 - BAR_WIDTH) / 2.0);
        assert_eq!(y, 1032.0 - BAR_HEIGHT - BAR_MARGIN);
    }

    #[test]
    fn respects_top_or_left_taskbar_and_secondary_monitor_offset() {
        // Secondary monitor to the left with taskbar docked at the top.
        let (x, y) = bar_origin((-1280.0, 40.0), (1280.0, 984.0));
        assert!(x >= -1280.0 && x + BAR_WIDTH <= 0.0);
        assert_eq!(y, 40.0 + 984.0 - BAR_HEIGHT - BAR_MARGIN);
    }

    #[test]
    fn tiny_screen_does_not_go_negative() {
        let (x, y) = bar_origin((0.0, 0.0), (200.0, 30.0));
        assert_eq!((x, y), (0.0, 0.0));
    }
}
