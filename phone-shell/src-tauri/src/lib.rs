//! The elesdee phone shell.
//!
//! The shell holds the native pieces: the link identity, the pairing handshake,
//! the computer list, the live connection and the QR scanner. The screens come
//! from `phone-shell/ui` and talk to this crate through a small command set.

mod commands;
mod forward;
mod store;

use commands::{
    phone_computers, phone_connect, phone_disconnect, phone_forget, phone_forward_start,
    phone_forward_stop, phone_info, phone_pair_cancel, phone_pair_finish, phone_pair_start,
    phone_request, phone_subscribe, phone_unsubscribe,
};
use tauri::Manager;

/// Starts the phone shell.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    // The scanner plugin is built only for mobile targets. The paste path
    // works everywhere, so the desktop build still runs for development.
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());

    let app = builder
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = commands::init_state(app.handle());
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            phone_info,
            phone_computers,
            phone_pair_start,
            phone_pair_finish,
            phone_pair_cancel,
            phone_forget,
            phone_connect,
            phone_disconnect,
            phone_request,
            phone_subscribe,
            phone_unsubscribe,
            phone_forward_start,
            phone_forward_stop,
        ])
        .build(tauri::generate_context!())
        .expect("the elesdee phone shell failed to start");

    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::Exit)
            && let Some(state) = handle.try_state::<commands::ShellState>()
        {
            commands::shutdown(&state);
        }
    });
}
