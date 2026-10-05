use std::borrow::Cow;

use gpui_kit::App;

/// Static instances of the same bundled Inter variable source. GPUI's macOS
/// font-kit matcher selects a face, not a position on its variable `wght` axis.
pub fn init(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!(
                "../../assets/product-fonts/TauInter-400.ttf"
            )),
            Cow::Borrowed(include_bytes!(
                "../../assets/product-fonts/TauInter-500.ttf"
            )),
            Cow::Borrowed(include_bytes!(
                "../../assets/product-fonts/TauInter-600.ttf"
            )),
            Cow::Borrowed(include_bytes!(
                "../../assets/product-fonts/TauInter-650.ttf"
            )),
            Cow::Borrowed(include_bytes!(
                "../../assets/product-fonts/TauInter-700.ttf"
            )),
        ])
        .expect("bundled TauInter weights");
}

pub fn family() -> &'static str {
    "TauInter"
}
