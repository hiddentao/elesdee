// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use tauri::Manager;

struct CoreProcess(Mutex<Option<Child>>);

impl Drop for CoreProcess {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.0.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
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

fn main() {
    let app = tauri::Builder::default()
        .setup(|app| {
            app.manage(CoreProcess(Mutex::new(start_core())));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("elesdee failed to start");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            let child = {
                let state = app_handle.state::<CoreProcess>();
                state.0.lock().ok().and_then(|mut guard| guard.take())
            };
            if let Some(mut child) = child {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    });
}
