//! The elesdee phone shell.
//!
//! The shell holds the native pieces: the link identity, the pairing handshake,
//! the computer list, the live connection and the QR scanner. The screens come
//! from `phone-shell/ui` and talk to this crate through a small command set.

mod commands;
mod forward;
mod notifications;
mod store;
mod ui;

use commands::{
    phone_computers, phone_connect, phone_disconnect, phone_forget, phone_forward_start,
    phone_forward_stop, phone_info, phone_pair_cancel, phone_pair_finish, phone_pair_start,
    phone_pick_upload, phone_read_image, phone_request, phone_subscribe, phone_ui_state,
    phone_unsubscribe, phone_upload,
};
use notifications::{
    phone_notification_event, phone_notification_request, phone_notification_show,
    phone_notification_state,
};
use tauri::Manager;
use ui::UiStore;

/// Blocks top-level navigation outside the shell and the UI package.
///
/// The hook runs for the webview created from the configuration, because
/// Tauri installs a navigation handler on every webview. A new-window request
/// is not a navigation; see the note in [`run`].
fn navigation_plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("elesdee-navigation")
        .on_navigation(|_webview, url| ui::navigation_allowed(url.as_str(), ui::PACKAGE_ORIGIN))
        .build()
}

/// Starts the phone shell.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    // The scanner plugin is built only for mobile targets. The paste path
    // works everywhere, so the desktop build still runs for development.
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());

    // Tauri 2.12 has no plugin hook for a new-window request. Its only handler
    // is a builder method on a webview this shell does not create in code, so
    // the platform webview answers the request. The macOS webview refuses it
    // when no handler is set, so no second webview opens.
    let app = builder
        .plugin(navigation_plugin())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        // The stored UI packages for the opened computer are served on their
        // own origin. Its current and previous builds stay reachable.
        .register_uri_scheme_protocol("elesdee", |context, request| {
            match context.app_handle().try_state::<UiStore>() {
                Some(store) => store.handle(request),
                None => ui::not_found_response(),
            }
        })
        .setup(|app| {
            let state = commands::init_state(app.handle());
            app.manage(state.ui.clone());
            app.manage(state);
            app.manage(notifications::NotificationRoutes::default());
            notifications::register_click_handler(app.handle());
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
            phone_read_image,
            phone_pick_upload,
            phone_upload,
            phone_subscribe,
            phone_ui_state,
            phone_unsubscribe,
            phone_forward_start,
            phone_forward_stop,
            phone_notification_state,
            phone_notification_request,
            phone_notification_show,
            phone_notification_event,
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
