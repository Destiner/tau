#[path = "../probes/highlight.rs"]
mod highlight;
#[path = "../probes/common.rs"]
mod probe;
#[path = "../probes/queue.rs"]
mod queue;
#[path = "../text.rs"]
mod text;

fn main() -> anyhow::Result<()> {
    probe::run::<queue::QueueProbe>()
}
