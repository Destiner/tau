//! Native OS descriptors for host applications to compose and install.

mod host;
mod lifecycle;
mod menus;
mod paths;
mod window;

pub use host::{HostCallbacks, NativeHost, NewSession, NewWindow, Quit, host_menus, reveal_window};
pub use lifecycle::{Intent, Lifecycle, LifecycleCallbacks, Outcome, Request};
pub use menus::edit_menu;
pub use paths::{PathChooser, PathFailure, PathOperation, PathOutcome, PathStart};
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
mod modal_tests;
#[cfg(test)]
mod tests;
