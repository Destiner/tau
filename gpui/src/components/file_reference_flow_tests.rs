use super::*;
use gpui_kit::base::text::{start_inline_flow_link_work, take_inline_flow_link_work};
use std::collections::{BTreeMap, BTreeSet};

struct FlowHost {
    text: Entity<TextViewState>,
    width: f32,
}

impl Render for FlowHost {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(self.width)).child(
            TextView::new(&self.text)
                .selection_format(gpui_kit::base::text::SelectionFormat::Source)
                .style(text_style(false, window))
                .markdown_extensions(extensions()),
        )
    }
}

#[test]
fn actual_flow_prepaint_indexes_fifty_thousand_distinct_paths_with_code_tail() {
    let mut source = (0..50_000)
        .map(|i| format!("src/f{i}.rs "))
        .collect::<String>();
    source.push_str("`tail`");
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    start_inline_flow_link_work();
    let mut window = app.open_window(|_, cx| FlowHost {
        text: cx.new(|cx| {
            TextViewState::markdown_with_extensions(&source, extensions(), cx)
                .selection_format(gpui_kit::base::text::SelectionFormat::Source)
        }),
        width: 640.,
    });
    let retained = window.read(|host, _| host.text.clone());
    window.draw();
    let work = take_inline_flow_link_work();
    assert!(
        work.fragments > 1_000,
        "must prepaint the actual wrapped flow"
    );
    let origins = work
        .controls
        .iter()
        .filter(|(_, _, first)| *first)
        .map(|(owner, range, _)| (*owner, range.start, range.end))
        .collect::<BTreeSet<_>>();
    assert_eq!(origins.len(), 50_000);
    // Counters charge actual range predicates in slicing and provenance recovery,
    // not callback admission or elapsed time. Index construction is per item,
    // not per fragment; repeated layout/render passes remain permitted.
    assert!(work.indexed_links >= 50_000);
    assert!(work.indexed_links < 50_000 * 10);
    assert!(work.lookup_steps < work.controls.len() * 100);
    assert!(work.highlight_steps < work.fragments * 100);
    eprintln!(
        "flow actual work: indexed={} lookup={} highlights={} fragments={} controls={} origins={}",
        work.indexed_links,
        work.lookup_steps,
        work.highlight_steps,
        work.fragments,
        work.controls.len(),
        origins.len()
    );
    window.update(|host, _, cx| host.text.update(cx, |text, cx| text.select_all(cx)));
    window.read(|host, cx| {
        assert_eq!(host.text, retained);
        assert_eq!(host.text.read(cx).selected_text().trim_end(), source);
    });
    start_inline_flow_link_work();
    window.draw();
    let next = take_inline_flow_link_work();
    let retained_origins = next
        .controls
        .iter()
        .filter(|(_, _, first)| *first)
        .map(|(owner, range, _)| (*owner, range.start, range.end))
        .collect::<BTreeSet<_>>();
    assert_eq!(origins, retained_origins);
}

#[test]
fn actual_flow_repeated_destinations_keep_distinct_origins_and_one_tab_stop_after_wrap() {
    for (source, width, expected) in [
        ("src/a.rs src/a.rs `tail`", 640., vec![0..8, 9..17]),
        (
            "[one](https://example.com) [two](https://example.com) `tail`",
            640.,
            vec![0..3, 4..7],
        ),
        (
            "[aaaaaaaaaaaaaaaaaaaaaaaaaaa](https://example.com) [aaaaaaaaaaaaaaaaaaaaaaaaaaa](https://example.com) `tail`",
            85.,
            vec![0..27, 28..55],
        ),
        (
            "src/aaaaaaaaaaaaaaaaaaaaaaaa.rs src/aaaaaaaaaaaaaaaaaaaaaaaa.rs `tail`",
            85.,
            vec![0..31, 32..63],
        ),
        ("prefix `src/a.rs src/a.rs` tail", 640., vec![7..15, 16..24]),
    ] {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let mut window = app.open_window(|_, cx| FlowHost {
            text: cx.new(|cx| TextViewState::markdown_with_extensions(source, extensions(), cx)),
            width,
        });
        window.activate_accessibility();
        start_inline_flow_link_work();
        window.draw();
        let work = take_inline_flow_link_work();
        let tree = window.accessibility_tree().unwrap();
        let links = tree
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == gpui_kit::accesskit::Role::Link)
            .collect::<Vec<_>>();
        assert_eq!(links.len(), 2);
        assert_ne!(links[0].0, links[1].0);
        let ax_ids = links.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>();
        window.draw();
        let retained_tree = window.accessibility_tree().unwrap();
        assert_eq!(
            ax_ids,
            retained_tree
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == gpui_kit::accesskit::Role::Link)
                .map(|(id, _)| *id)
                .collect()
        );
        let mut firsts = BTreeMap::new();
        let mut continuations = 0;
        for (_, range, first) in &work.controls {
            if *first {
                *firsts.entry((range.start, range.end)).or_insert(0) += 1;
            } else {
                continuations += 1;
            }
        }
        assert_eq!(
            firsts,
            expected
                .iter()
                .map(|range| ((range.start, range.end), 1))
                .collect()
        );
        if width < 100. {
            assert!(
                continuations >= 2,
                "each repeated destination must really wrap"
            );
        }
        // Traverse the real rendered focus tree, not manufactured focus handles.
        window.update(|_, window, cx| window.focus_next(cx));
        let first = window.update(|_, window, cx| window.focused(cx));
        window.update(|_, window, cx| window.focus_next(cx));
        let second = window.update(|_, window, cx| window.focused(cx));
        assert!(first.is_some() && second.is_some());
        assert_ne!(first, second);
        window.update(|_, window, cx| window.focus_next(cx));
        window.update(|_, window, cx| assert_eq!(window.focused(cx), first));
        let retained = window.read(|host, _| host.text.clone());
        window.update(|host, _, cx| {
            host.text
                .update(cx, |text, cx| text.set_text("src/b.rs `tail`", cx))
        });
        start_inline_flow_link_work();
        window.draw();
        let updated = take_inline_flow_link_work();
        assert_eq!(
            updated
                .controls
                .iter()
                .filter(|(_, _, first)| *first)
                .count(),
            1
        );
        window.read(|host, _| assert_eq!(host.text, retained));
    }
}
