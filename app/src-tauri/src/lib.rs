od audio;
mod commands;
mod playlist;
mod skin;

use parking_lot::RwLock;
use std::sync::Arc;
use tauri::{Emitter, Manager, PhysicalSize, Size};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};

pub struct AppInner {
    pub player: Arc<RwLock<audio::PlayerState>>,
    pub playlist: Arc<RwLock<playlist::Playlist>>,
    pub eq_gains: Arc<RwLock<[f32; 10]>>,
    /// Artboard CSS scale (0.5 = classic 750x1030). Native-owned so Ctrl+/-
    /// works even when WebView2 swallows DOM key events.
    pub ui_scale: Arc<RwLock<f64>>,
}

pub const ART_W: f64 = 1500.0;
pub const ART_H: f64 = 2060.0;
const FIT_MARGIN_X: f64 = 16.0;
const FIT_MARGIN_Y: f64 = 24.0;
pub const SCALE_PRESETS: [f64; 9] = [0.25, 0.30, 0.35, 0.40, 0.45, 0.50, 0.60, 0.75, 1.0];

#[derive(Clone, serde::Serialize)]
pub struct UiScaleInfo {
    pub scale: f64,
    pub w: u32,
    pub h: u32,
    pub max_scale: f64,
}

fn max_scale_for_monitor(monitor: &tauri::Monitor) -> f64 {
    let dpi = monitor.scale_factor().max(0.1);
    let work = monitor.work_area();
    let work_w = f64::from(work.size.width);
    let work_h = f64::from(work.size.height);
    // Work area is physical px; window physical size = ART * scale * dpi.
    let max_s = ((work_h - FIT_MARGIN_Y * dpi) / (ART_H * dpi))
        .min((work_w - FIT_MARGIN_X * dpi) / (ART_W * dpi));
    max_s.clamp(0.25, 1.0)
}

fn pick_default_scale(max_s: f64) -> f64 {
    if max_s >= 0.5 {
        0.5
    } else {
        max_s.min(0.375).max(0.25)
    }
}

fn current_max_scale(app: &tauri::AppHandle) -> f64 {
    app.get_webview_window("main")
        .and_then(|w| w.current_monitor().ok().flatten())
        .map(|m| max_scale_for_monitor(&m))
        .unwrap_or(0.5)
}

/// Resize the main window to the artboard scale and notify the frontend.
pub fn apply_ui_scale(app: &tauri::AppHandle, requested: f64) -> Result<UiScaleInfo, String> {
    let state = app.state::<AppInner>();
    let win = app
        .get_webview_window("main")
        .ok_or_else(|| "main window missing".to_string())?;
    let monitor = win
        .current_monitor()
        .map_err(|e| e.to_string())?
        .or_else(|| win.primary_monitor().ok().flatten())
        .ok_or_else(|| "no monitor".to_string())?;

    let dpi = monitor.scale_factor().max(0.1);
    let max_s = max_scale_for_monitor(&monitor);
    let s = requested.clamp(0.25, max_s.min(1.0));

    let logical_w = ART_W * s;
    let logical_h = ART_H * s;
    // PhysicalSize avoids LogicalSize/DPI mismatches on Windows WebView2.
    let phys_w = (logical_w * dpi).round() as u32;
    let phys_h = (logical_h * dpi).round() as u32;

    let min_w = (ART_W * 0.25 * dpi).round() as u32;
    let min_h = (ART_H * 0.25 * dpi).round() as u32;
    let max_w = (ART_W * max_s * dpi).round() as u32;
    let max_h = (ART_H * max_s * dpi).round() as u32;
    let _ = win.set_min_size(Some(Size::Physical(PhysicalSize::new(min_w, min_h))));
    let _ = win.set_max_size(Some(Size::Physical(PhysicalSize::new(max_w, max_h))));
    win.set_size(Size::Physical(PhysicalSize::new(phys_w, phys_h)))
        .map_err(|e| e.to_string())?;
    let _ = win.center();

    *state.ui_scale.write() = s;
    let info = UiScaleInfo {
        scale: s,
        w: logical_w.round() as u32,
        h: logical_h.round() as u32,
        max_scale: max_s,
    };
    let _ = app.emit("ui_scale", info.clone());
    Ok(info)
}

pub fn cycle_ui_scale(app: &tauri::AppHandle, direction: i32) -> Result<UiScaleInfo, String> {
    let state = app.state::<AppInner>();
    let current = *state.ui_scale.read();
    let max_s = current_max_scale(app);

    let mut presets: Vec<f64> = SCALE_PRESETS
        .iter()
        .copied()
        .filter(|p| *p <= max_s + 0.02)
        .collect();
    if presets.is_empty() {
        presets.push((max_s * 100.0).round() / 100.0);
    } else if (presets[presets.len() - 1] - max_s).abs() > 0.03 {
        presets.push((max_s * 100.0).round() / 100.0);
    }

    let idx = presets
        .iter()
        .position(|p| (*p - current).abs() < 0.02)
        .unwrap_or_else(|| {
            presets
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    (**a - current)
                        .abs()
                        .partial_cmp(&(**b - current).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0)
        });

    let next = idx as i32 + direction;
    if next < 0 || next >= presets.len() as i32 {
        return apply_ui_scale(app, current);
    }
    apply_ui_scale(app, presets[next as usize])
}

fn handle_zoom_shortcut(app: &tauri::AppHandle, code: Code) {
    match code {
        Code::Equal | Code::NumpadAdd => {
            let _ = cycle_ui_scale(app, 1);
        }
        Code::Minus | Code::NumpadSubtract => {
            let _ = cycle_ui_scale(app, -1);
        }
        Code::Digit0 | Code::Numpad0 => {
            let max_s = current_max_scale(app);
            let _ = apply_ui_scale(app, pick_default_scale(max_s));
        }
        Code::KeyD => {
            let state = app.state::<AppInner>();
            let cur = *state.ui_scale.read();
            let max_s = current_max_scale(app);
            let target = if cur >= 0.9 {
                pick_default_scale(max_s)
            } else {
                1.0_f64.min(max_s)
            };
            let _ = apply_ui_scale(app, target);
        }
        _ => {}
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    handle_zoom_shortcut(app, shortcut.key);
                })
                .build(),
        )
        .manage(AppInner {
            player: Arc::new(RwLock::new(audio::PlayerState::default())),
            playlist: Arc::new(RwLock::new(playlist::Playlist::default())),
            eq_gains: Arc::new(RwLock::new([0.0; 10])),
            ui_scale: Arc::new(RwLock::new(0.5)),
        })
        .setup(|app| {
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                // OS-level hotkeys: WebView2 often swallows Ctrl+/- as browser
                // accelerators before the page sees them (Windows).
                let shortcuts = [
                    Shortcut::new(Some(Modifiers::CONTROL), Code::Equal),
                    Shortcut::new(Some(Modifiers::CONTROL), Code::NumpadAdd),
                    Shortcut::new(
                        Some(Modifiers::CONTROL | Modifiers::SHIFT),
                        Code::Equal,
                    ),
                    Shortcut::new(Some(Modifiers::CONTROL), Code::Minus),
                    Shortcut::new(Some(Modifiers::CONTROL), Code::NumpadSubtract),
                    Shortcut::new(Some(Modifiers::CONTROL), Code::Digit0),
                    Shortcut::new(Some(Modifiers::CONTROL), Code::KeyD),
                ];
                for sc in shortcuts {
                    if let Err(e) = app.global_shortcut().register(sc) {
                        log::warn!("zoom shortcut register failed: {e}");
                    }
                }
            }

            let max_s = current_max_scale(&app.handle().clone());
            let s = pick_default_scale(max_s);
            if let Err(e) = apply_ui_scale(&app.handle().clone(), s) {
                log::warn!("initial ui scale failed: {e}");
            }

            let handle = app.handle().clone();
            audio::spawn_spectrum_task(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_files,
            commands::play,
            commands::pause,
            commands::stop,
            commands::next,
            commands::prev,
            commands::play_index,
            commands::seek,
            commands::get_position,
            commands::set_volume,
            commands::set_eq,
            commands::set_params,
            commands::get_playlist,
            commands::reorder_playlist,
            commands::clear_playlist,
            commands::load_skin,
            commands::resize_window_px,
            commands::set_ui_scale,
            commands::cycle_ui_scale,
            commands::get_ui_scale,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                audio::player::shutdown();
            }
        });

    // Let WebView finish teardown, then kill leftover audio threads
    // so `tauri dev` / the shell is not left stuck (Chrome_WidgetWin noise).
    audio::player::shutdown();
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::process::exit(0);
}
