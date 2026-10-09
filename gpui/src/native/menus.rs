use gpui_kit::{
    Menu, MenuItem, OsAction,
    base::input::{Copy, Cut, Paste, Redo, SelectAll, Undo},
};

/// Compose with the host's application menus; routing stays with the focused responder.
pub fn edit_menu() -> Menu {
    Menu::new("Edit").items([
        MenuItem::os_action("Undo", Undo, OsAction::Undo),
        MenuItem::os_action("Redo", Redo, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action("Cut", Cut, OsAction::Cut),
        MenuItem::os_action("Copy", Copy, OsAction::Copy),
        MenuItem::os_action("Paste", Paste, OsAction::Paste),
        MenuItem::separator(),
        MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
    ])
}
