#[path = "../probes/menus.rs"]
mod menus;
#[path = "../probes/common.rs"]
mod probe;
#[path = "../text.rs"]
mod text;

fn main() -> anyhow::Result<()> {
    probe::run::<menus::MenusProbe>()
}
