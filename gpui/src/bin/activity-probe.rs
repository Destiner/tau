#[path = "../probes/activity.rs"]
mod activity;
#[path = "../probes/common.rs"]
mod probe;
#[path = "../text.rs"]
mod text;

fn main() -> anyhow::Result<()> {
    probe::run::<activity::ActivityProbe>()
}
