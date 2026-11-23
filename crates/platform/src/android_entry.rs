//! NativeActivity entry glue. Use `android_entrypoint!` from the game.

use std::cell::RefCell;

use crate::PlatformError;

pub use winit::platform::android::activity::AndroidApp;

thread_local! {
    static APP: RefCell<Option<AndroidApp>> = const { RefCell::new(None) };
}

/// Installs the native activity for the event loop started by the entrypoint.
pub fn with_app(app: AndroidApp, run: impl FnOnce()) {
    APP.set(Some(app));
    run();
    APP.set(None);
}

pub(crate) fn take_app() -> Result<AndroidApp, PlatformError> {
    APP.take().ok_or_else(|| {
        PlatformError::EventLoop("Android requires a NativeActivity entrypoint".into())
    })
}

/// Exports the Android NativeActivity entrypoint from a game's shared library.
///
/// The supplied function takes no arguments and starts the platform loop on
/// the current thread. Desktop targets still use their ordinary `main`.
#[macro_export]
macro_rules! android_entrypoint {
    ($run:path) => {
        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        fn android_main(app: $crate::android_entry::AndroidApp) {
            $crate::android_entry::with_app(app, $run);
        }
    };
}
