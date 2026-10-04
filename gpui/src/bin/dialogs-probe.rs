#[path = "../probes/dialogs.rs"]
mod dialogs;
#[path = "../probes/common.rs"]
mod probe;
#[path = "../text.rs"]
mod text;

fn main() -> anyhow::Result<()> {
    probe::run::<dialogs::DialogsProbe>()
}
