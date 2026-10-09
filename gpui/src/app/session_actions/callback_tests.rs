use super::super::tests::{create, rpc, with_shell};
use super::*;
use serde_json::json;
fn take(requests: &async_channel::Receiver<Command>, kind: &str) -> serde_json::Value {
    std::iter::from_fn(|| requests.try_recv().ok())
        .find_map(|c| match c {
            Command::Send { request, .. } if request["type"] == kind => Some(request),
            _ => None,
        })
        .unwrap()
}
#[test]
fn pi_echo_registry_callback_and_retry_remain_bound_after_switch() {
    with_shell(128, |s, w, cx, requests| {
        super::tests::fixture(s, w, cx);
        s.session_action(
            &Action::Rename {
                session_id: "live".into(),
                title: "Requested".into(),
            },
            w,
            cx,
        );
        let rename = take(&requests, "set_session_name");
        create(s, "other", "other draft", w, cx);
        s.active = "other".into();
        s.ingest(rpc("live",1,json!({"type":"response","id":rename["id"],"command":"set_session_name","success":true})),cx);
        let read = take(&requests, "get_state");
        s.ingest(rpc("live",1,json!({"type":"response","id":read["id"],"command":"get_state","success":true,"data":{"sessionId":"saved","sessionFile":"/saved","sessionName":"Echoed"}})),cx);
        let stored = std::iter::from_fn(|| requests.try_recv().ok())
            .find(|c| matches!(c,Command::StoreName {name,..} if name=="Echoed"))
            .unwrap();
        if let Command::StoreName {
            runtime,
            generation,
            revision,
            id,
            path,
            name,
            ..
        } = stored
        {
            assert_eq!(runtime, "live");
            assert_eq!(generation, 1);
            s.ingest(
                Event::NameStored {
                    runtime: runtime.clone(),
                    generation,
                    revision,
                    id: id.clone(),
                    path: path.clone(),
                    name: name.clone(),
                    success: false,
                },
                cx,
            );
            assert!(s.session_action_error().is_none());
            assert_eq!(
                s.workspace.as_ref().unwrap().projects[0].sessions[0].title,
                "Original"
            );
            s.active = "live".into();
            assert!(s.session_action_error().is_some());
            s.retry_session_action(w, cx);
            assert!(
                std::iter::from_fn(|| requests.try_recv().ok())
                    .any(|c| matches!(c,Command::StoreName {name,..} if name=="Echoed"))
            );
            let latest = s.session_actions.names[&runtime];
            // A delayed failure from the original save cannot override this retry.
            s.finish_name(
                runtime.clone(),
                generation,
                revision,
                id.clone(),
                path.clone(),
                name.clone(),
                false,
            );
            assert!(s.session_action_error().is_none());
            s.ingest(
                Event::NameStored {
                    runtime,
                    generation,
                    revision: latest,
                    id,
                    path,
                    name,
                    success: true,
                },
                cx,
            );
            assert!(s.session_action_error().is_none());
            assert_eq!(
                s.workspace.as_ref().unwrap().projects[0].sessions[0].title,
                "Echoed"
            );
            assert_eq!(s.sessions["other"].controller.state().draft, "other draft");
            assert_eq!(s.sessions["live"].controller.state().draft, "sacred");
        }
    });
}
