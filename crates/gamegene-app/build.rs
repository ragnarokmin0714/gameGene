//! Build script: embed the Windows executable icon resource.
//!
//! `with_icon` in `main.rs` only sets the running window's icon. The taskbar,
//! Explorer, and pinned shortcuts read the icon from the .exe resource, so it
//! is embedded here at build time. No-op on every non-Windows target.
fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            // Don't fail the build if the resource compiler is unavailable;
            // the app still runs, just without the embedded exe icon.
            println!("cargo:warning=failed to embed Windows icon: {e}");
        }
    }
}
