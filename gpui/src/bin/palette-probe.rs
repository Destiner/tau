#[path = "../probes/palette.rs"]
mod palette;
#[path = "../probes/common.rs"]
mod probe;
#[path = "../text.rs"]
mod text;

fn main() -> anyhow::Result<()> {
    probe::run::<palette::PaletteProbe>()
}
