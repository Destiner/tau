use super::*;
use gpui_kit::base::SelectionFormat;
use gpui_kit::{MouseButton, TestApp, test::TestWindowExt};

#[test]
fn embedded_diagram_first_click_drag_escape_and_reopen_keep_real_viewer_focus() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let mut view = None;
    let mut window = app.open_window(|w, cx| {
        let markdown =
            cx.new(|cx| MarkdownView::new("```mermaid\ngraph TD\nA --> B\n```", false, |_| {}, cx));
        view = Some(markdown.clone());
        gpui_kit::base::Root::new(markdown, w, cx)
    });
    let view = view.unwrap();
    window.simulate_resize(gpui_kit::size(px(900.), px(700.)));
    window.update(|_, w, cx| w.bounds_changed(cx));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        app.run_until_parked();
        window.draw();
        if window.update(|_, w, cx| {
            w.try_find("diagram-expand").is_some()
                && !view.read(cx).presentation.read(cx).render_pending(cx)
        }) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "diagram preparation");
        std::thread::sleep(Duration::from_millis(10));
        app.advance_clock(Duration::from_millis(10));
    }
    let handle = app.windows()[0];
    app.update(|cx| {
        cx.update_window(handle, |_, w, cx| w.click("diagram-expand", cx))
            .unwrap()
    });
    app.update(|cx| {
        cx.update_window(handle, |_, w, cx| {
            let from = w.find("diagram-fullscreen").bounds().center();
            let to = w.find("diagram-expand").bounds().center();
            w.drag(from, to, cx);
            w.press("escape", cx);
            assert!(w.try_find("diagram-fullscreen").is_none());
            let diagrams = view.read(cx).presentation.read(cx).diagram_blocks();
            assert!(diagrams[0].read(cx).expand_focused(w));
            assert!(!diagrams[0].read(cx).is_expanded());
            w.click("diagram-expand", cx);
            assert!(diagrams[0].read(cx).is_expanded());
            w.click("diagram-fullscreen", cx);
            assert!(!diagrams[0].read(cx).is_expanded());
            assert!(diagrams[0].read(cx).expand_focused(w));
        })
        .unwrap()
    });
}

#[test]
fn configured_initial_parse_and_cold_copy_share_offsets() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let source = "first\r\nsecond\n\n$money$ and ~~deleted~~\n\n| | |\n|---|---|\n|alpha|beta|";
    let mut window = app.open_window(|_, cx| MarkdownView::new(source, false, |_| {}, cx));
    // Check before first paint: configuration must precede the synchronous initial parse.
    window.read(|view, cx| {
        let mounted = view.text.read(cx).rendered_text();
        let cold = TextViewState::copy_markdown_slice_with_extensions(
            source,
            None,
            None,
            SelectionFormat::Plain,
            &extensions(),
        )
        .unwrap();
        assert_eq!(mounted.as_str(), cold);
        assert!(cold.contains("first\nsecond"));
        assert!(cold.contains("$money$"));
        for end in cold
            .char_indices()
            .map(|(offset, _)| offset)
            .chain([cold.len()])
        {
            let prefix = TextViewState::copy_markdown_slice_with_extensions(
                source,
                None,
                Some(end),
                SelectionFormat::Plain,
                &extensions(),
            )
            .unwrap();
            assert_eq!(prefix, &cold[..end]);
        }
    });
    window.draw();
}

#[test]
fn copy_uses_parsed_tilde_fences_and_exact_vue_newline() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let mut window = app.open_window(|_, cx| {
        MarkdownView::new(
            "~~~txt\none ``` literal\n~~~\n\n````txt\ntwo\n\n````",
            false,
            |_| {},
            cx,
        )
    });
    window.draw();
    for (id, expected, selected) in [("copy-1", "one ``` literal\n", 0), ("copy-2", "two\n", 1)] {
        let button = window.update(|_, w, _| w.find(id).bounds());
        window.simulate_click(button.center(), MouseButton::Left);
        window.read(|view, _| assert_eq!(view.copied_block(), Some(selected)));
        window.update(|_, _, cx| {
            assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), expected)
        });
    }
    // Source updates keep the same owned text state, not a new view/selection runtime.
    let before = window.read(|view, _| view.text.clone());
    window.update(|view, _, cx| view.set_source("~~~txt\nchanged\n~~~", cx));
    assert_eq!(window.read(|view, _| view.text.clone()), before);
}

#[test]
fn source_append_retains_owned_logical_selection_and_text_entity() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let mut window =
        app.open_window(|_, cx| MarkdownView::new("stable first\n", false, |_| {}, cx));
    window.draw();
    let text = window.read(|view, _| view.text.clone());
    window.update(|view, _, cx| {
        view.text.update(cx, |text, cx| {
            text.set_logical_selection(Some(0..6), cx).unwrap()
        });
        view.set_source("stable first\n\nnew paragraph", cx);
    });
    app.run_until_parked();
    window.draw();
    window.read(|view, cx| {
        assert_eq!(view.text, text);
        assert_eq!(view.text.read(cx).logical_selection(), Some(0..6));
        assert_eq!(view.text.read(cx).selected_text_with(cx), "stable");
    });
}

#[test]
fn font_relative_style_has_no_fixture_padding_and_matches_queue_scale() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let mut window = app.open_window(|_, cx| MarkdownView::new("# Heading", false, |_| {}, cx));
    window.update(|_, window, cx| {
        assert_eq!(
            gpui_kit::base::Theme::global(cx).tokens.typography.mono,
            support::mono()
        );
        for body in [11., 13.] {
            let style = text_style_at_size(false, body, window);
            assert_eq!(style.body_text_size(), Some(px(body)));
            // Individual heading rules outrank Vue's adjacent-heading :where() selector.
            assert_eq!(style.block_spacing().unwrap().consecutive_heading_top, 1.2);
            assert_eq!(style.block_spacing().unwrap().first_code_top, 0.7);
            assert_eq!(style.task_checkbox().unwrap().offset_em, 0.0);
            assert!(style.heading_strong_inherit());
            assert!(style.deleted_muted());
            assert_eq!(style.inline_code_scale(), 0.88);
        }
    });
}

#[test]
fn code_copy_button_is_keyboard_focusable_without_mouseout_focus_transfer() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let mut window =
        app.open_window(|_, cx| MarkdownView::new("~~~txt\nkeyboard\n~~~", false, |_| {}, cx));
    window.draw();
    let button = window.update(|_, w, _| w.find("copy-1").bounds());
    window.update(|view, _, cx| view.text.update(cx, |text, cx| text.select_all(cx)));
    let selection = window.read(|view, cx| view.text.read(cx).selected_text());
    assert!(!selection.is_empty());
    window.simulate_click(button.center(), MouseButton::Left);
    window.read(|view, cx| assert_eq!(view.text.read(cx).selected_text(), selection));
    window.simulate_keystroke("enter");
    assert_eq!(window.read(|view, _| view.copied_block()), Some(0));
    let focus = window.update(|_, w, cx| w.focused(cx));
    assert!(focus.is_some());
    window.simulate_mouse_move(gpui_kit::point(px(500.), px(500.)));
    window.draw();
    window.update(|_, w, cx| assert_eq!(w.focused(cx), focus));
    app.advance_clock(Duration::from_millis(1201));
    app.run_until_parked();
    assert_eq!(window.read(|view, _| view.copied_block()), None);
    // Keyboard activation still works after hover/feedback expires.
    use gpui_kit::{KeyDownEvent, KeyUpEvent, Keystroke};
    let keystroke = Keystroke::parse("space").unwrap();
    window.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    window.simulate_event(KeyUpEvent { keystroke });
    assert_eq!(window.read(|view, _| view.copied_block()), Some(0));
}

#[test]
fn natural_height_diagram_admission_does_not_churn_when_cache_is_full() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let source = "~~~mermaid\nnot a diagram\n~~~\n\n".repeat(40);
    let mut window =
        app.open_window(|_, cx| MarkdownView::new(source, false, |_| {}, cx).natural_height());
    window.draw();
    app.run_until_parked();
    let admitted = window.read(|view, cx| {
        let p = view.presentation.read(cx);
        assert_eq!(p.diagrams.len(), 32);
        p.diagrams
            .iter()
            .map(|entry| entry.entity.clone())
            .collect::<Vec<_>>()
    });
    for _ in 0..3 {
        window.draw();
        app.run_until_parked();
        window.read(|view, cx| {
            let p = view.presentation.read(cx);
            assert!(p.pending.is_empty());
            assert_eq!(
                p.diagrams
                    .iter()
                    .map(|entry| entry.entity.clone())
                    .collect::<Vec<_>>(),
                admitted
            );
        });
    }
}

#[test]
fn mermaid_hook_only_prepares_closed_first_token_bounded_blocks() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let oversized = "😀".repeat(4001);
    let source = format!(
        "```mermaid\nflowchart LR\nA-->B\n```\n\n~~~mermaid extra\nflowchart LR\nC-->D\n~~~\n\n```rust mermaid\nnot a diagram\n```\n\n```mermaid\n{oversized}\n```\n\n```mermaid\nflowchart LR\nE-->F"
    );
    let mut window = app.open_window(|_, cx| MarkdownView::new(source, false, |_| {}, cx));
    app.run_until_parked();
    window.draw();
    app.run_until_parked();
    window.read(|view, cx| {
        let presentation = view.presentation.read(cx);
        assert_eq!(presentation.diagrams.len(), 2);
        assert!(
            presentation
                .diagrams
                .iter()
                .all(|entry| entry.source.encode_utf16().count() <= 8000)
        );
        assert!(presentation.pending.is_empty());
    });
}
