use crate::audio::player;
use crate::playlist::PlaylistEntry;
use crate::skin::{parse_skin_path, LoadedSkin};
use crate::AppInner;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub fn open_files(paths: Vec<String>, state: State<'_, AppInner>) -> Result<usize, String> {
    let mut pl = state.playlist.write();
    let before = pl.len();
    pl.add_paths(&paths);
    // Duration is optional and must not decode the whole file (UI freeze).
    for entry in pl.entries_mut()[before..].iter_mut() {
        if let Ok(meta) = crate::audio::decoder::duration_hint(std::path::Path::new(&entry.path)) {
            if meta > 0.0 {
                let m = (meta / 60.0).floor() as u64;
                let s = (meta % 60.0).floor() as u64;
                entry.duration = Some(format!("{m:02}:{s:02}"));
            } else {
                entry.duration = Some("--:--".into());
            }
        }
    }
    Ok(pl.len() - before)
}

fn spawn_load(path: String, app: AppHandle) {
    let gen = player::prepare_load();
    std::thread::spawn(move || {
        match player::load_and_play_gen(std::path::Path::new(&path), gen) {
            Ok(()) => {
                let _ = app.emit("play_started", ());
            }
            Err(e) => {
                log::error!("load failed: {e}");
                let _ = app.emit("error", format!("Load failed: {e}"));
            }
        }
    });
}

fn play_index_inner(
    state: &State<'_, AppInner>,
    index: usize,
    app: &AppHandle,
) -> Result<Option<u64>, String> {
    let path = {
        let mut pl = state.playlist.write();
        pl.set_current(Some(index));
        pl.current_path().map(|s| s.to_string())
    };
    let Some(path) = path else {
        return Ok(None);
    };
    // Optimistic: UI lights play + spectrum while decode runs
    let _ = app.emit("play_started", ());
    spawn_load(path, app.clone());
    Ok(state.playlist.read().current_id())
}

#[tauri::command]
pub fn play_index(index: usize, state: State<'_, AppInner>, app: AppHandle) -> Result<(), String> {
    if let Some(id) = play_index_inner(&state, index, &app)? {
        let _ = app.emit("track_changed", id);
    }
    Ok(())
}

#[tauri::command]
pub fn play(state: State<'_, AppInner>, app: AppHandle) -> Result<(), String> {
    let has_audio = !player::shared().samples.read().is_empty();
    if has_audio {
        player::play();
        let _ = app.emit("play_started", ());
        return Ok(());
    }
    // Nothing decoded yet — load current (or first) track then play
    let len = state.playlist.read().len();
    if len == 0 {
        return Err("playlist is empty — OPEN a file first".into());
    }
    let idx = state.playlist.read().current.unwrap_or(0).min(len - 1);
    if let Some(id) = play_index_inner(&state, idx, &app)? {
        let _ = app.emit("track_changed", id);
    }
    Ok(())
}

#[tauri::command]
pub fn pause() {
    player::pause();
}

#[tauri::command]
pub fn stop() {
    player::stop();
}

fn step_track(state: &State<'_, AppInner>, app: &AppHandle, delta: isize) -> Result<(), String> {
    let len = state.playlist.read().len();
    if len == 0 {
        return Err("empty playlist".into());
    }
    let cur = state.playlist.read().current;
    let next = match cur {
        None => 0,
        Some(c) => {
            let mut n = (c as isize) + delta;
            if n < 0 {
                n = len as isize - 1;
            } else if n >= len as isize {
                n = 0;
            }
            n as usize
        }
    };
    let path = {
        let mut pl = state.playlist.write();
        pl.set_current(Some(next));
        pl.current_path().map(|s| s.to_string())
    };
    let id = state.playlist.read().current_id();
    if let Some(path) = path {
        spawn_load(path, app.clone());
    }
    if let Some(id) = id {
        let _ = app.emit("track_changed", id);
    }
    Ok(())
}

#[tauri::command]
pub fn next(state: State<'_, AppInner>, app: AppHandle) -> Result<(), String> {
    log::info!("next track");
    step_track(&state, &app, 1)
}

#[tauri::command]
pub fn prev(state: State<'_, AppInner>, app: AppHandle) -> Result<(), String> {
    log::info!("prev track");
    step_track(&state, &app, -1)
}

#[tauri::command]
pub fn seek(seconds: f64) {
    player::seek_secs(seconds);
}

#[tauri::command]
pub fn get_position() -> f64 {
    player::position_secs()
}

#[tauri::command]
pub fn set_volume(volume: f64) {
    player::set_volume(volume as f32);
}

#[tauri::command]
pub fn set_eq(gains: Vec<f64>, state: State<'_, AppInner>) -> Result<(), String> {
    if gains.len() != 10 {
        return Err("expected 10 EQ gains".into());
    }
    let mut arr = [0.0f32; 10];
    for (i, g) in gains.iter().enumerate() {
        arr[i] = *g as f32;
    }
    *state.eq_gains.write() = arr;
    player::set_eq(arr);
    Ok(())
}

#[tauri::command]
pub fn set_params(
    volume: f64,
    pitch: f64,
    reverb: f64,
    eq: Vec<f64>,
    speed: f64,
) -> Result<(), String> {
    if eq.len() != 10 {
        return Err("expected 10 EQ gains".into());
    }
    let mut arr = [0.0f32; 10];
    for (i, g) in eq.iter().enumerate() {
        arr[i] = *g as f32;
    }
    player::set_params(
        volume as f32,
        pitch as f32,
        reverb as f32,
        arr,
        speed as f32,
    );
    Ok(())
}

#[tauri::command]
pub fn get_playlist(state: State<'_, AppInner>) -> Vec<PlaylistEntry> {
    state.playlist.read().entries().to_vec()
}

#[tauri::command]
pub fn reorder_playlist(from: usize, to: usize, state: State<'_, AppInner>) -> Result<(), String> {
    state.playlist.write().reorder(from, to);
    Ok(())
}

#[tauri::command]
pub fn clear_playlist(state: State<'_, AppInner>) {
    player::stop();
    state.playlist.write().clear();
}

#[tauri::command]
pub fn resize_window_px(
    w: u32,
    h: u32,
    app: AppHandle,
) -> Result<(), String> {
    use tauri::{LogicalSize, Manager};
    if let Some(win) = app.get_webview_window("main") {
        win.set_size(LogicalSize::new(w as f64, h as f64))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Apply an artboard CSS scale (0.25–1.0). Clamped to the monitor work area.
#[tauri::command]
pub fn set_ui_scale(scale: f64, app: AppHandle) -> Result<crate::UiScaleInfo, String> {
    crate::apply_ui_scale(&app, scale)
}

/// Step through zoom presets. direction: 1 = zoom in, -1 = zoom out.
#[tauri::command]
pub fn cycle_ui_scale(direction: i32, app: AppHandle) -> Result<crate::UiScaleInfo, String> {
    crate::cycle_ui_scale(&app, direction)
}

#[tauri::command]
pub fn get_ui_scale(app: AppHandle) -> Result<crate::UiScaleInfo, String> {
    let state = app.state::<crate::AppInner>();
    let scale = *state.ui_scale.read();
    let max_scale = app
        .get_webview_window("main")
        .and_then(|w| w.current_monitor().ok().flatten())
        .map(|m| {
            let dpi = m.scale_factor().max(0.1);
            let work = m.work_area();
            let work_w = f64::from(work.size.width);
            let work_h = f64::from(work.size.height);
            let max_s = ((work_h - 24.0 * dpi) / (crate::ART_H * dpi))
                .min((work_w - 16.0 * dpi) / (crate::ART_W * dpi));
            max_s.clamp(0.25, 1.0)
        })
        .unwrap_or(0.5);
    Ok(crate::UiScaleInfo {
        scale,
        w: (crate::ART_W * scale).round() as u32,
        h: (crate::ART_H * scale).round() as u32,
        max_scale,
    })
}

#[tauri::command]
pub fn load_skin(path: String) -> Result<serde_json::Value, String> {
    let loaded: LoadedSkin =
        parse_skin_path(PathBuf::from(&path).as_path()).map_err(|e| e.to_string())?;
    let mut assets = serde_json::Map::new();
    for (rel, bytes) in &loaded.assets {
        if rel.ends_with(".png") || rel.ends_with(".webp") {
            use base64::Engine as _;
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            let mime = if rel.ends_with(".webp") {
                "image/webp"
            } else {
                "image/png"
            };
            assets.insert(rel.clone(), serde_json::Value::String(format!("data:{mime};base64,{b64}")));
        }
    }
    Ok(serde_json::json!({
        "manifest": loaded.manifest,
        "assets": assets,
    }))
}
