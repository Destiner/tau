//! Native OS descriptors for host applications to compose and install.

mod menus;
mod window;

pub use menus::edit_menu;
pub use window::{CloseWindow, MinimizeWindow, install_window_commands, window_menu};

#[cfg(test)]
mod accessibility_tests;
#[cfg(test)]
mod ime_controls_tests;
#[cfg(test)]
mod ime_tests;
#[cfg(test)]
mod menu_availability_tests;
#[cfg(test)]
mod tests;
