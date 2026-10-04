#[path = "../probes/models.rs"]
mod models;
#[path = "../probes/common.rs"]
mod probe;
#[path = "../text.rs"]
mod text;

fn main() -> anyhow::Result<()> {
    probe::run::<models::ModelsProbe>()
}
