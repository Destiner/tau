pub mod composer;
pub mod dialogs;
pub mod font;
pub mod markdown;
pub mod menus;
pub mod models;
pub mod palette;
pub mod queue;
pub mod support;
pub mod syntax;

use std::borrow::Cow;

use gpui_kit::{
    App, AssetSource, SharedString,
    base::{Theme, ThemeAppearance},
};

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        Ok(support::asset(path))
    }

    fn list(&self, _: &str) -> gpui_kit::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

pub fn init(cx: &mut App, dark: bool) {
    crate::text::init();
    gpui_kit::init(cx);
    Theme::global_mut(cx).appearance = if dark {
        ThemeAppearance::Dark
    } else {
        ThemeAppearance::Light
    };
    font::init(cx);
}
