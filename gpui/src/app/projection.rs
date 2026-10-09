use crate::components::{
    activity::ActivityContent,
    transcript::{Transcript, TranscriptContent, TranscriptEntry},
};
use gpui_kit::{Context, Entity};
use tau_session::transcript::Entry;

#[derive(Default)]
pub struct Projection {
    previous: Vec<Revision>,
}
struct Revision {
    id: String,
    revision: u64,
    length: usize,
}
enum Change<'a> {
    Reconcile(&'a [Entry]),
    Append(&'a Entry),
    Replace(&'a Entry),
    Text(&'a str, &'a str),
}
fn row(entry: &Entry) -> TranscriptEntry {
    let content = match entry.kind.as_str() {
        "user" => TranscriptContent::User(entry.text.clone()),
        "thinking" => TranscriptContent::Activity(ActivityContent::Thinking {
            text: entry.text.clone(),
        }),
        "tool" => TranscriptContent::Activity(ActivityContent::Tool {
            name: entry.tool_name.clone().unwrap_or_else(|| "Tool".into()),
            argument: entry.text.clone(),
            arguments: entry.arguments.to_string(),
            result: entry.result.clone(),
            running: entry.activity,
            failed: entry.is_error,
        }),
        _ => TranscriptContent::Assistant(entry.text.clone()),
    };
    TranscriptEntry {
        id: entry.id.clone(),
        content,
    }
}
impl Projection {
    // Only IDs and integer metadata are inspected for unchanged rows. The controller
    // certifies append lineage so even the active prose prefix need not be hashed.
    fn changes<'a>(&mut self, source: &'a [Entry], mut apply: impl FnMut(Change<'a>)) {
        let structural = self.previous.len() > source.len()
            || self
                .previous
                .iter()
                .zip(source)
                .any(|(old, entry)| old.id != entry.id);
        if structural {
            apply(Change::Reconcile(source));
            self.previous.clear();
        }
        for (index, entry) in source.iter().enumerate() {
            if let Some(old) = self.previous.get_mut(index) {
                if old.revision == entry.revision {
                    continue;
                }
                if matches!(entry.kind.as_str(), "assistant" | "user")
                    && entry.text_reset_revision <= old.revision
                    && entry.text.len() >= old.length
                    && entry.text.is_char_boundary(old.length)
                {
                    apply(Change::Text(&entry.id, &entry.text[old.length..]));
                } else {
                    apply(Change::Replace(entry));
                }
                old.revision = entry.revision;
                old.length = entry.text.len();
            } else {
                if !structural {
                    apply(Change::Append(entry));
                }
                self.previous.push(Revision {
                    id: entry.id.clone(),
                    revision: entry.revision,
                    length: entry.text.len(),
                });
            }
        }
    }
    pub fn sync<T: 'static>(
        &mut self,
        source: &[Entry],
        transcript: &Entity<Transcript>,
        cx: &mut Context<T>,
    ) {
        self.changes(source, |change| {
            transcript.update(cx, |t, cx| match change {
                Change::Reconcile(entries) => t
                    .set_entries(entries.iter().map(row).collect(), cx)
                    .expect("controller transcript IDs must be unique"),
                Change::Append(entry) => t
                    .append_entry(row(entry), cx)
                    .expect("controller transcript IDs must be unique"),
                Change::Replace(entry) => {
                    assert!(t.update_content(&entry.id, row(entry).content, cx));
                }
                Change::Text(id, suffix) => {
                    assert!(t.append_text(id, suffix, cx));
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(id: usize, kind: &str, text: &str) -> Entry {
        serde_json::from_value(json!({
            "id": id.to_string(), "kind": kind, "text": text,
            "activity": false, "pending": false, "interrupted": false,
            "tool_call_id": null, "tool_name": null, "arguments": null,
            "result": "", "is_error": false, "source": id.to_string()
        }))
        .unwrap()
    }
    #[test]
    fn large_history_projects_only_changed_row_and_prose_suffix() {
        let mut source: Vec<_> = (0..2000)
            .map(|i| entry(i, "assistant", &"history".repeat(1000)))
            .collect();
        source.push(entry(2000, "tool", ""));
        let mut projection = Projection::default();
        projection.changes(&source, |_| {});
        source[2000].revision += 1;
        source[2000].result = "output".into();
        let mut projected = 0;
        let mut copied = 0;
        projection.changes(&source, |change| match change {
            Change::Replace(entry) => {
                projected += 1;
                let row = row(entry);
                let TranscriptContent::Activity(ActivityContent::Tool { result, .. }) = row.content
                else {
                    panic!()
                };
                copied += result.len();
            }
            _ => panic!("unchanged history must not be projected"),
        });
        assert_eq!((projected, copied), (1, 6));
        source[1999].text.push_str(" delta");
        source[1999].revision += 1;
        projection.changes(&source, |change| match change {
            Change::Text(id, suffix) => {
                assert_eq!(id, "1999");
                assert_eq!(suffix, " delta");
            }
            _ => panic!("only the new suffix should reach the renderer"),
        });
        source[1999].text = "replacement".into();
        source[1999].revision += 1;
        source[1999].text_reset_revision = source[1999].revision;
        projection.changes(&source, |change| {
            assert!(matches!(change, Change::Replace(_)))
        });
    }
}
