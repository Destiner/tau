//! Native OS descriptors for host applications to compose and install.

mod menus;

pub use menus::edit_menu;

#[cfg(test)]
mod menu_availability_tests;
#[cfg(test)]
mod tests;
