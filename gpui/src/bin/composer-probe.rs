#[path = "../probes/composer.rs"]
mod composer;
#[path = "../probes/common.rs"]
mod probe;
#[path = "../text.rs"]
mod text;

fn main() -> anyhow::Result<()> {
    probe::run::<composer::ComposerProbe>()
}
