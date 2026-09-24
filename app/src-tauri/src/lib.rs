mod audio;
mod commands;
mod playlist;
mod skin;

use parking_lot::RwLock;
use std::sync::Arc;

pub struct AppInner {
    pub player: Arc<RwLock<audio::PlayerState>>,
    pub playlist: Arc<RwLock<playlist::Playlist>>,
    pub eq_gains: Arc<RwLock<[f32; 10]>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppInner {
            player: Arc::new(RwLock::new(audio::PlayerState::default())),
            playlist: Arc::new(RwLock::new(playlist::Playlist::default())),
            eq_gains: Arc::new(RwLock::new([0.0; 10])),
        })
        .setup(|app| {
            use tauri::{LogicalSize, Manager};
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_size(LogicalSize::new(750.0, 1030.0));
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
