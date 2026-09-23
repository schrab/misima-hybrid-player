use crate::audio::player;
use crate::playlist::PlaylistEntry;
use crate::skin::{parse_skin_path, LoadedSkin};
use crate::AppInner;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};

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

fn play_index_inner(state: &State<'_, AppInner>, index: usize) -> Result<Option<u64>, String> {
    let path = {
        let mut pl = state.playlist.write();
        pl.set_current(Some(index));
        pl.current_path().map(|s| s.to_string())
    };
    let Some(path) = path else {
        return Ok(None);
    };
    // Decode off the command thread so the UI never freezes on large MP3s.
    let handle = std::thread::spawn(move || {
        player::load_and_play(std::path::Path::new(&path)).map_err(|e| e.to_string())
    });
    // Do not join here — stream starts when decode completes.
    // Surface errors via a detached logger; UI stays responsive.
    std::thread::spawn(move || {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::error!("load failed: {e}"),
            Err(_) => log::error!("load thread panicked"),
        }
    });
    let id = state.playlist.read().current_id();
    Ok(id)
}

#[tauri::command]
pub fn play_index(index: usize, state: State<'_, AppInner>, app: AppHandle) -> Result<(), String> {
    if let Some(id) = play_index_inner(&state, index)? {
        let _ = app.emit("track_changed", id);
    }
    Ok(())
}

#[tauri::command]
pub fn play(state: State<'_, AppInner>, app: AppHandle) -> Result<(), String> {
    {
        let pl = state.playlist.read();
        if pl.current.is_some() && player::shared().samples.read().is_empty() {
            drop(pl);
            let idx = state.playlist.read().current.unwrap_or(0);
            if let Some(id) = play_index_inner(&state, idx)? {
                let _ = app.emit("track_changed", id);
            }
            return Ok(());
        }
        if pl.current.is_none() && !pl.is_empty() {
            drop(pl);
            if let Some(id) = play_index_inner(&state, 0)? {
                let _ = app.emit("track_changed", id);
            }
            return Ok(());
        }
    }
    player::play();
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
        return Ok(());
    }
    let cur = state.playlist.read().current;
    let next = match cur {
        None => 0,
        Some(c) => {
            let mut n = c as isize + delta;
            if n < 0 {
                n = len as isize - 1;
            } else if n >= len as isize {
                n = 0;
            }
            n as usize
        }
    };
    if let Some(id) = play_index_inner(&state, next)? {
        let _ = app.emit("track_changed", id);
    }
    Ok(())
}

#[tauri::command]
pub fn next(state: State<'_, AppInner>, app: AppHandle) -> Result<(), String> {
    step_track(&state, &app, 1)
}

#[tauri::command]
pub fn prev(state: State<'_, AppInner>, app: AppHandle) -> Result<(), String> {
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
