// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_keepawake::{KeepAwakeConfig, TauriPluginKeepawakeExt};

mod notifications;
mod updater;

const AUTOSTART_ARG: &str = "--autostart";

struct CoreProcess(Mutex<Option<Child>>);

#[derive(Default)]
struct QuitState {
    confirmed: AtomicBool,
}

#[derive(Default)]
struct AwakeState(AtomicBool);

/// What the close button does. A hidden window keeps the core, its sessions
/// and the phone connection alive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowCloseAction {
    Hide,
    AskToQuit,
}

/// macOS keeps the process and its sessions alive while the window is hidden.
/// Other platforms have no hidden-window mode, so a close keeps the confirmed
/// quit path.
const fn platform_keeps_alive_without_window() -> bool {
    cfg!(target_os = "macos")
}

const fn window_close_action(keeps_alive_without_window: bool) -> WindowCloseAction {
    if keeps_alive_without_window {
        WindowCloseAction::Hide
    } else {
        WindowCloseAction::AskToQuit
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartAtLoginState {
    enabled: bool,
}

#[tauri::command]
fn get_start_at_login(app: tauri::AppHandle) -> Result<StartAtLoginState, String> {
    let enabled = app
        .autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())?;
    Ok(StartAtLoginState { enabled })
}

#[tauri::command]
fn set_start_at_login(app: tauri::AppHandle, enabled: bool) -> Result<StartAtLoginState, String> {
    if enabled {
        app.autolaunch()
            .enable()
            .map_err(|error| error.to_string())?;
    } else {
        app.autolaunch()
            .disable()
            .map_err(|error| error.to_string())?;
    }
    let actual = app
        .autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())?;
    Ok(StartAtLoginState { enabled: actual })
}

#[tauri::command]
fn set_keep_awake(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    let state = app.state::<AwakeState>();
    if enabled {
        if !state.0.load(Ordering::SeqCst) {
            // Clear a stale registration before adding the new one.
            let _ = app.tauri_plugin_keepawake().stop(&app);
            app.tauri_plugin_keepawake()
                .start(
                    &app,
                    Some(KeepAwakeConfig {
                        display: false,
                        idle: true,
                        sleep: true,
                    }),
                )
                .map_err(|error| error.to_string())?;
            state.0.store(true, Ordering::SeqCst);
        }
    } else if state.0.swap(false, Ordering::SeqCst) {
        app.tauri_plugin_keepawake()
            .stop(&app)
            .map_err(|error| error.to_string())?;
    }
    Ok(state.0.load(Ordering::SeqCst))
}

fn launched_at_login<I, S>(arguments: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    arguments
        .into_iter()
        .any(|argument| argument.as_ref() == AUTOSTART_ARG)
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    let state = app.state::<QuitState>();
    state.confirmed.store(true, Ordering::SeqCst);
    app.exit(0);
}

fn stop_core(child: &mut Child) {
    #[cfg(unix)]
    {
        // Ask the core to save its state and stop, then wait before forcing it.
        unsafe {
            libc::kill(child.id() as i32, libc::SIGTERM);
        }
        for _ in 0..50 {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(_) => break,
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Saves the sessions and stops the core before its bundle is replaced.
pub(crate) fn stop_core_for_update(app: &tauri::AppHandle) {
    let state = app.state::<CoreProcess>();
    let child = state.0.lock().ok().and_then(|mut guard| guard.take());
    if let Some(mut child) = child {
        stop_core(&mut child);
    }
}

/// Starts the core again when an update could not be installed.
pub(crate) fn start_core_after_update(app: &tauri::AppHandle) {
    let state = app.state::<CoreProcess>();
    let mut guard = match state.0.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    if guard.is_none() {
        *guard = start_core();
    }
}

impl Drop for CoreProcess {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.0.lock() {
            if let Some(mut child) = guard.take() {
                stop_core(&mut child);
            }
        }
    }
}

fn core_binary() -> PathBuf {
    if let Ok(path) = std::env::var("ELESDEE_CORE_BIN") {
        return PathBuf::from(path);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("elesdee-core");
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from("core/target/debug/elesdee-core")
}

fn start_core() -> Option<Child> {
    let path = core_binary();
    match Command::new(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => Some(child),
        Err(error) => {
            eprintln!(
                "elesdee: could not start the core service at {}: {error}",
                path.display()
            );
            None
        }
    }
}

fn checked_dir(path: &str) -> Result<PathBuf, String> {
    let folder = PathBuf::from(path);
    if !folder.is_absolute() {
        return Err("The folder path must be absolute.".to_owned());
    }
    if !folder.is_dir() {
        return Err("The folder does not exist.".to_owned());
    }
    Ok(folder)
}

#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    let folder = checked_dir(&path)?;
    open_with(&folder, false)
}

#[tauri::command]
fn open_terminal(path: String) -> Result<(), String> {
    let folder = checked_dir(&path)?;
    open_with(&folder, true)
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !is_loopback_url(&url) {
        return Err("Only a loopback address can be opened.".to_owned());
    }
    opener(false)
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open the address: {error}"))
}

/// Returns true for the local demo-server address that the app shows.
fn is_loopback_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let Some((host, port)) = rest.rsplit_once(':') else {
        return false;
    };
    if !matches!(host, "localhost" | "127.0.0.1") {
        return false;
    }
    port.parse::<u16>().is_ok_and(|port| port > 0)
}

fn open_with(folder: &std::path::Path, terminal: bool) -> Result<(), String> {
    opener(terminal)
        .arg(folder)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open the folder: {error}"))
}

fn opener(terminal: bool) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        if terminal {
            command.args(["-a", "Terminal"]);
        }
        command
    }
    #[cfg(target_os = "windows")]
    {
        if terminal {
            let mut command = Command::new("cmd");
            command.args(["/C", "start", "cmd", "/K", "cd", "/d"]);
            return command;
        }
        Command::new("explorer")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut command = Command::new(if terminal {
            "x-terminal-emulator"
        } else {
            "xdg-open"
        });
        if terminal {
            command.arg("--working-directory");
        }
        command
    }
}

fn main() {
    let at_login = launched_at_login(std::env::args().skip(1));
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .plugin(tauri_plugin_keepawake::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_window_event(|window, event| {
            let tauri::WindowEvent::CloseRequested { api, .. } = event else {
                return;
            };
            api.prevent_close();
            match window_close_action(platform_keeps_alive_without_window()) {
                WindowCloseAction::Hide => {
                    let _ = window.hide();
                }
                WindowCloseAction::AskToQuit => {
                    let _ = window.app_handle().emit("elesdee://quit-requested", ());
                }
            }
        })
        .manage(QuitState::default())
        .manage(AwakeState::default())
        .invoke_handler(tauri::generate_handler![
            open_folder,
            open_terminal,
            open_url,
            quit_app,
            get_start_at_login,
            set_start_at_login,
            set_keep_awake,
            notifications::notification_permission_state,
            notifications::request_notification_permission,
            notifications::show_notification,
            updater::get_update_state,
            updater::check_for_update,
            updater::defer_update,
            updater::install_update
        ])
        .setup(move |app| {
            notifications::register_click_handler(app.handle());
            app.manage(CoreProcess(Mutex::new(start_core())));
            updater::setup(app.handle());
            if !at_login {
                show_main_window(app.handle());
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("elesdee failed to start");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            let state = app_handle.state::<QuitState>();
            if !state.confirmed.load(Ordering::SeqCst) {
                api.prevent_exit();
                // The confirmation lives in the window, so show it first when
                // it is hidden.
                show_main_window(app_handle);
                let _ = app_handle.emit("elesdee://quit-requested", ());
            }
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen {
            has_visible_windows,
            ..
        } => {
            if !has_visible_windows {
                show_main_window(app_handle);
            }
        }
        tauri::RunEvent::Exit => {
            let child = {
                let state = app_handle.state::<CoreProcess>();
                state.0.lock().ok().and_then(|mut guard| guard.take())
            };
            if let Some(mut child) = child {
                stop_core(&mut child);
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_autostart_argument() {
        assert!(launched_at_login([AUTOSTART_ARG]));
        assert!(launched_at_login(["--other", AUTOSTART_ARG]));
        assert!(!launched_at_login(["--other"]));
        assert!(!launched_at_login(std::iter::empty::<&str>()));
    }

    #[test]
    fn a_window_close_hides_the_window_when_the_app_keeps_running() {
        assert_eq!(window_close_action(true), WindowCloseAction::Hide);
    }

    #[test]
    fn a_window_close_asks_to_quit_when_the_window_cannot_hide_alone() {
        assert_eq!(window_close_action(false), WindowCloseAction::AskToQuit);
    }
}
