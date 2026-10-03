/// Opt into browser-like grayscale antialiasing before either interactive or headless rendering.
#[cfg(target_os = "macos")]
pub fn init() {
    gpui_macos::use_antialiased_text();
}

#[cfg(not(target_os = "macos"))]
pub fn init() {}
