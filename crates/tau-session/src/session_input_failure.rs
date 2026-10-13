use super::*;

impl Controller {
    /// Input failure is not an output-producer fence. Keep final records admitted.
    pub fn input_closed(&mut self, generation: u64) -> Vec<Effect> {
        if generation != self.generation || !self.connected {
            return vec![];
        }
        self.reconciliation.input_closed = true;
        self.state.ready = false;
        self.state.retry = None;
        self.admission = None;
        self.fail("Pi is no longer accepting requests. Final output is still being received.");
        self.update_status();
        vec![]
    }

    /// Transport hosts use this admission path to keep input closure sticky
    /// across delayed state replies, without losing completed-output proof.
    pub fn ingest_output(&mut self, runtime: &str, generation: u64, record: Value) -> Vec<Effect> {
        let effects = self.ingest(runtime, generation, record);
        if self.reconciliation.input_closed {
            self.state.ready = false;
        }
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_closure_admits_final_records_but_exit_and_stale_generations_do_not() {
        for input_only in [true, false] {
            let mut c = Controller::new("r");
            c.bind_source("b", "/b.jsonl");
            let reads = c.begin_generation(7);
            let state = reads
                .iter()
                .find_map(|effect| match effect {
                    Effect::Send(request) if request["type"] == "get_state" => {
                        Some(request.clone())
                    }
                    _ => None,
                })
                .unwrap();
            c.ingest_output("r", 7, json!({"type":"response", "command":"get_state", "id":state["id"],
                "success":true, "data":{"sessionId":"b", "sessionFile":"/b.jsonl", "isStreaming":false}}));
            assert!(c.state.ready);
            c.input_closed(6);
            assert!(c.state.ready, "stale input closure must be ignored");
            let refresh = c.refresh();
            if input_only {
                c.input_closed(7);
            } else {
                c.transport_failed(7);
            }
            assert!(!c.state.ready);
            for effect in refresh {
                if let Effect::Send(request) = effect {
                    c.ingest_output("r", 7, json!({"type":"response", "command":"get_state", "id":request["id"],
                        "success":true, "data":{"sessionId":"b", "sessionFile":"/b.jsonl", "isStreaming":false}}));
                }
            }
            assert!(!c.state.ready, "delayed state response cannot reopen input");
            assert!(c.submit("unsafe").is_empty());
            let final_record = json!({"type":"message_end", "message":{"role":"assistant",
                "content":[{"type":"text", "text":"final"}], "stopReason":"stop"}});
            c.ingest_output("r", 6, final_record.clone());
            assert!(c.state.transcript.is_empty());
            c.ingest_output("r", 7, final_record);
            assert_eq!(
                c.state.transcript.iter().any(|row| row.text == "final"),
                input_only
            );
            let proof = c.prepare_exit();
            assert_eq!(
                proof.iter().any(
                    |effect| matches!(effect, Effect::RegisterSession { id, .. } if id == "b")
                ),
                input_only
            );
            c.transport_failed(7);
            let before = c.state.transcript.len();
            c.ingest_output(
                "r",
                7,
                json!({"type":"message_end", "message":{"role":"assistant",
                "content":[{"type":"text", "text":"too late"}], "stopReason":"stop"}}),
            );
            assert_eq!(c.state.transcript.len(), before);
            c.begin_generation(8);
            assert!(
                !c.reconciliation.input_closed,
                "successor has independent input admission"
            );
        }
    }
}
