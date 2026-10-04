#[path = "../probes/files.rs"]
mod files;
#[path = "../probes/common.rs"]
mod probe;
#[path = "../text.rs"]
mod text;

fn main() -> anyhow::Result<()> {
    probe::run::<files::FilesProbe>()
}
