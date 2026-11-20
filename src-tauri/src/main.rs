// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{Emitter, Manager};

struct CoreProcess(Mutex<Option<Child>>);

#[derive(Default)]
struct QuitState {
    confirmed: AtomicBool,
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
                Err(_) => break
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
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

fn open_with(folder: &std::path::Path, terminal: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        if terminal {
            command.args(["-a", "Terminal"]);
        }
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("explorer");
        if terminal {
            command = Command::new("cmd");
            command.args(["/C", "start", "cmd", "/K", "cd", "/d"]).arg(folder);
            return command
                .spawn()
                .map(|_| ())
                .map_err(|error| format!("Could not open a terminal: {error}"));
        }
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new(if terminal { "x-terminal-emulator" } else { "xdg-open" });
        if terminal {
            command.arg("--working-directory");
        }
        command
    };

    command
        .arg(folder)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open the folder: {error}"))
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(QuitState::default())
        .invoke_handler(tauri::generate_handler![
            open_folder,
            open_terminal,
            quit_app
        ])
        .setup(|app| {
            app.manage(CoreProcess(Mutex::new(start_core())));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("elesdee failed to start");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            let state = app_handle.state::<QuitState>();
            if !state.confirmed.load(Ordering::SeqCst) {
                api.prevent_exit();
                let _ = app_handle.emit("elesdee://quit-requested", ());
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
