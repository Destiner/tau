#[path = "../probes/highlight.rs"]
mod highlight;
#[path = "../probes/markdown.rs"]
mod markdown;
#[path = "../probes/common.rs"]
mod probe;
#[path = "../text.rs"]
mod text;

fn main() -> anyhow::Result<()> {
    probe::run::<markdown::MarkdownProbe>()
}
