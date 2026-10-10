use crate::components::{
    activity::ActivityContent,
    transcript::{Transcript, TranscriptContent, TranscriptEntry},
};
use gpui_kit::{Context, Entity};
use tau_session::{session::State, transcript::Entry};

/// Vue App.vue: stopping wins, compaction and an assistant tail hide working.
pub fn working_indicator(state: &State) -> Option<&'static str> {
    if state.stopping {
        Some("Stopping")
    } else if state.working
        && !state.compacting
        && state
            .transcript
            .last()
            .is_none_or(|entry| entry.kind != "assistant")
    {
        Some("Working")
    } else {
        None
    }
}

#[derive(Default)]
pub struct Projection {
    previous: Vec<Revision>,
}
struct Revision {
    id: String,
    kind: String,
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
            name: entry
                .tool_name
                .clone()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "tool".into()),
            argument: entry.tool_description(),
            arguments: entry.tool_arguments_text(),
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
                if old.kind == entry.kind
                    && matches!(entry.kind.as_str(), "assistant" | "user")
                    && entry.text_reset_revision <= old.revision
                    && entry.text.len() >= old.length
                    && entry.text.is_char_boundary(old.length)
                {
                    apply(Change::Text(&entry.id, &entry.text[old.length..]));
                } else {
                    apply(Change::Replace(entry));
                }
                old.kind.clone_from(&entry.kind);
                old.revision = entry.revision;
                old.length = entry.text.len();
            } else {
                if !structural {
                    apply(Change::Append(entry));
                }
                self.previous.push(Revision {
                    id: entry.id.clone(),
                    kind: entry.kind.clone(),
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
    fn production_projection_matches_vue_activity_geometry_and_remeasures_neighbors() {
        let mut projection = Projection::default();
        crate::components::transcript::tests::production_projection_matches_vue_activity_geometry_and_remeasures_neighbors(
            |source, transcript, cx| projection.sync(source, transcript, cx),
        );
    }

    #[test]
    fn production_state_projection_renders_transient_working_tail() {
        let mut projection = Projection::default();
        crate::components::transcript::tests::production_working_tail(|state, transcript, cx| {
            projection.sync(&state.transcript, transcript, cx);
            transcript.update(cx, |this, cx| {
                this.set_working_indicator(working_indicator(state), cx)
            });
        });
    }

    #[test]
    fn working_indicator_matches_vue_and_controller_generation_fences() {
        use tau_session::session::Controller;
        let mut controller = Controller::new("working");
        controller.begin_generation(1);
        assert_eq!(working_indicator(controller.state()), None);
        controller.handle(json!({"type":"compaction_start"}));
        assert_eq!(working_indicator(controller.state()), None);
        controller.handle(json!({"type":"compaction_end"}));
        assert_eq!(working_indicator(controller.state()), None);
        controller.handle(json!({"type":"agent_start"}));
        assert_eq!(working_indicator(controller.state()), Some("Working"));
        controller.handle(json!({"type":"compaction_start"}));
        assert!(controller.state().compacting);
        assert_eq!(working_indicator(controller.state()), None);
        controller.handle(json!({"type":"compaction_end"}));
        assert_eq!(working_indicator(controller.state()), Some("Working"));
        let mut state = controller.state().clone();
        for kind in ["tool", "thinking", "user", "assistant"] {
            state.transcript = vec![entry(0, kind, "content")];
            assert_eq!(
                working_indicator(&state),
                if kind == "assistant" {
                    None
                } else {
                    Some("Working")
                }
            );
        }
        state.stopping = true;
        state.compacting = true;
        state.working = false;
        assert_eq!(working_indicator(&state), Some("Stopping"));
        controller.handle(json!({"type":"agent_settled"}));
        assert_eq!(working_indicator(controller.state()), None);
        assert!(!controller.state().compacting);
        controller.handle(json!({"type":"compaction_start"}));
        controller.begin_generation(2);
        controller.ingest("working", 1, json!({"type":"agent_start"}));
        assert_eq!(working_indicator(controller.state()), None);
        assert!(!controller.state().compacting);
        controller.handle(json!({"type":"agent_start"}));
        controller.transport_failed(2);
        assert_eq!(working_indicator(controller.state()), None);
        assert!(!controller.state().compacting);
    }

    #[test]
    fn kind_changes_replace_instead_of_using_prose_append_lineage() {
        let mut source = vec![entry(0, "thinking", "")];
        let mut projection = Projection::default();
        projection.changes(&source, |_| {});
        for kind in ["assistant", "user", "tool", "assistant"] {
            source[0].kind = kind.into();
            source[0].revision += 1;
            projection.changes(&source, |change| {
                assert!(matches!(change, Change::Replace(_)));
            });
        }
    }

    #[test]
    fn tool_headers_use_vue_semantics_without_mutating_raw_payloads() {
        let cases = [
            (
                "read",
                json!({"path":"gpui/APP.md", "offset":2}),
                "gpui/APP.md",
            ),
            ("bash", json!({"command":"bun run build"}), "bun run build"),
            (
                "edit",
                json!({"path":"file.rs", "oldText":"before", "newText":"after"}),
                "file.rs",
            ),
            ("write", json!({"file_path":"new.rs"}), "new.rs"),
            ("search", json!({"query":"needle"}), "needle"),
            (
                "custom",
                json!({"command":"", "path":23, "query":"fallback"}),
                "fallback",
            ),
            ("custom", json!([1, 2]), "[1,2]"),
            ("custom", json!("partial JSON"), "\"partial JSON\""),
            ("custom", json!({}), "{}"),
            ("", json!(null), ""),
        ];
        for (name, args, expected) in cases {
            let mut entry = entry(0, "tool", "");
            entry.tool_name = Some(name.into());
            entry.arguments = args.clone();
            entry.result = "raw\noutput  \n".into();
            let TranscriptContent::Activity(ActivityContent::Tool {
                name: label,
                argument,
                arguments,
                result,
                ..
            }) = row(&entry).content
            else {
                panic!()
            };
            assert_eq!(label, if name.is_empty() { "tool" } else { name });
            assert_eq!(argument, expected);
            if args.as_object().is_some_and(|args| !args.is_empty()) {
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&arguments).unwrap(),
                    args
                );
                assert!(arguments.contains('\n'));
            } else {
                assert!(arguments.is_empty());
            }
            assert_eq!(entry.arguments, args);
            assert_eq!(result, "raw\noutput  \n");
        }
    }

    #[test]
    fn production_controller_argument_updates_reach_the_same_tool_row() {
        use tau_session::session::Controller;
        let mut controller = Controller::new("projection");
        controller.begin_generation(1);
        controller.handle(json!({"type":"message_start", "message":{
            "role":"assistant", "timestamp":1, "content":[
                {"type":"toolCall", "id":"call", "name":"read"}
            ]
        }}));
        let mut projection = Projection::default();
        projection.changes(&controller.state().transcript, |_| {});
        let id = controller.state().transcript[0].id.clone();
        controller.handle(json!({"type":"tool_execution_start", "toolCallId":"call",
            "toolName":"read", "args":{"path":"gpui/APP.md"}}));
        let mut replaced = false;
        projection.changes(&controller.state().transcript, |change| {
            let Change::Replace(entry) = change else {
                panic!("argument changes must replace only the tool presentation")
            };
            assert_eq!(entry.id, id);
            let TranscriptContent::Activity(ActivityContent::Tool { argument, .. }) =
                row(entry).content
            else {
                panic!()
            };
            assert_eq!(argument, "gpui/APP.md");
            replaced = true;
        });
        assert!(replaced);
        controller.handle(
            json!({"type":"tool_execution_end", "toolCallId":"call", "toolName":"read",
            "result":{"content":[{"type":"text", "text":"raw result"}]}, "isError":false}),
        );
        assert_eq!(
            controller.state().transcript[0].tool_description(),
            "gpui/APP.md"
        );
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
