//! NativeActivity shared library for the clear-window example.

#![cfg(target_os = "android")]

mod clear_window;

engine_app::platform::android_entrypoint!(run);

fn run() {
    if let Err(error) = clear_window::main() {
        eprintln!("{error}");
    }
}
