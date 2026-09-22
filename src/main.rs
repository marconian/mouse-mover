#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

#[cfg(windows)]
mod native;
mod scheduler;

fn main() {
    #[cfg(windows)]
    if let Err(error) = native::run() {
        native::show_error(&error.to_string());
        std::process::exit(1);
    }

    #[cfg(not(windows))]
    {
        eprintln!("Mouse Mover requires Windows 10 or later.");
        std::process::exit(1);
    }
}
