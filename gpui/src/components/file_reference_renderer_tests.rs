use super::*;

#[path = "file_reference_flow_tests.rs"]
mod flow_tests;
use gpui_kit::base::{
    ElementExt,
    text::{InlineLinkTarget, MarkdownInlineLeaf, SourceSegment},
};
use gpui_kit::{MouseButton, TestApp, point, test::TestWindowExt};
use std::sync::{Arc, Mutex};

#[test]
fn large_production_paragraph_maps_every_path_with_bounded_endpoint_work() {
    let source = "src/a.rs ".repeat(50_000);
    let segments = [SourceSegment {
        rendered: 0..source.len(),
        source: 0..source.len(),
    }];
    let leaf = MarkdownInlineLeaf {
        text: &source,
        source: &source,
        segments: &segments,
        inline_code: false,
    };
    file_reference::MAPPING_STEPS.with(|steps| steps.set(0));
    let links = file_reference::inline_links(&leaf);
    assert_eq!(source.len(), 450_000);
    assert_eq!(links.len(), 50_000);
    for (index, link) in links.iter().enumerate() {
        assert_eq!(link.range, index * 9..index * 9 + 8);
        assert_eq!(link.path.as_ref(), "src/a.rs");
    }
    // One forward segment walk plus two logarithmic endpoint lookups per path.
    file_reference::MAPPING_STEPS.with(|steps| assert!(steps.get() < 3_000_000));
}

#[test]
fn entity_heavy_segments_are_walked_once_with_grapheme_offsets_unchanged() {
    let source = "&#233;👩‍💻 src/a.rs ".repeat(10_000);
    let text = "é👩‍💻 src/a.rs ".repeat(10_000);
    let mut segments = Vec::new();
    let mut rendered = 0;
    let mut written = 0;
    while rendered < text.len() {
        let c = text[rendered..].chars().next().unwrap();
        let bytes = if c == 'é' {
            "&#233;".len()
        } else {
            c.len_utf8()
        };
        segments.push(SourceSegment {
            rendered: rendered..rendered + c.len_utf8(),
            source: written..written + bytes,
        });
        rendered += c.len_utf8();
        written += bytes;
    }
    let leaf = MarkdownInlineLeaf {
        text: &text,
        source: &source,
        segments: &segments,
        inline_code: false,
    };
    file_reference::MAPPING_STEPS.with(|steps| steps.set(0));
    let links = file_reference::inline_links(&leaf);
    assert_eq!(links.len(), 10_000);
    for link in links {
        assert_eq!(&text[link.range], "src/a.rs");
    }
    file_reference::MAPPING_STEPS.with(|steps| assert!(steps.get() < 1_000_000));
}

#[test]
fn actual_large_paragraph_renderer_retains_state_and_normal_copy() {
    struct LargeHost {
        text: Entity<TextViewState>,
        admitted: Arc<Mutex<usize>>,
    }
    impl Render for LargeHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let admitted = self.admitted.clone();
            TextView::new(&self.text).markdown_extensions(extensions().inline_links(move |leaf| {
                let links = file_reference::inline_links(leaf);
                *admitted.lock().unwrap() += links.len();
                links
            }))
        }
    }
    let source = "src/a.rs ".repeat(50_000);
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let mut window = app.open_window(|_, cx| LargeHost {
        text: cx.new(|cx| TextViewState::markdown_with_extensions(&source, extensions(), cx)),
        admitted: Arc::new(Mutex::new(0)),
    });
    let retained = window.read(|host, _| host.text.clone());
    file_reference::MAPPING_STEPS.with(|steps| steps.set(0));
    window.draw();
    window.read(|host, cx| {
        assert_eq!(host.text, retained);
        assert!(*host.admitted.lock().unwrap() >= 50_000);
        assert_eq!(
            host.text.read(cx).rendered_text().as_str().trim_end(),
            source.trim_end()
        );
    });
    // Multiple render passes are permitted; each must use the bounded mapper.
    let paths = window.read(|host, _| *host.admitted.lock().unwrap());
    file_reference::MAPPING_STEPS.with(|steps| assert!(steps.get() < paths * 60));
    window.update(|host, _, cx| host.text.update(cx, |text, cx| text.select_all(cx)));
    window.read(|host, cx| {
        assert_eq!(host.text, retained);
        assert_eq!(
            host.text.read(cx).selected_text().trim_end(),
            source.trim_end()
        );
    });
}

#[test]
fn rendered_entity_provenance_and_inline_code_use_html_equivalent_ranges() {
    for (source, text, inline_code, expected) in [
        (
            "/tmp/a &#38; b.txt:42",
            "/tmp/a & b.txt:42",
            false,
            Some("/tmp/a & b.txt"),
        ),
        ("src&#47;a.rs", "src/a.rs", false, None),
        ("`src&#47;a.rs`", "src&#47;a.rs", true, None),
        ("&lt;src/a.rs&gt;", "<src/a.rs>", false, None),
    ] {
        let mut segments = Vec::new();
        let mut rendered = 0;
        let mut written = 0;
        if inline_code {
            written = 1;
        }
        while rendered < text.len() {
            let c = text[rendered..].chars().next().unwrap();
            let end = if !inline_code && source[written..].starts_with('&') {
                written + source[written..].find(';').unwrap() + 1
            } else {
                written + c.len_utf8()
            };
            segments.push(SourceSegment {
                rendered: rendered..rendered + c.len_utf8(),
                source: written..end,
            });
            written = end;
            rendered += c.len_utf8();
        }
        let leaf = MarkdownInlineLeaf {
            text,
            source,
            segments: &segments,
            inline_code,
        };
        let links = file_reference::inline_links(&leaf);
        assert_eq!(
            links.first().map(|link| link.path.as_ref()),
            expected,
            "{source}"
        );
        if let Some(link) = links.first() {
            assert_eq!(&text[link.range.clone()], expected.unwrap());
        }
    }
}

#[test]
fn native_context_target_preserves_selection_source_and_return_focus() {
    for (source, expected) in [
        ("src/a.rs:42", LinkTarget::WrittenPath("src/a.rs".into())),
        (
            "[file](docs/a%2520b.md)",
            LinkTarget::DocumentPath("docs/a%20b.md".into()),
        ),
        (
            "[web](https://example.com)",
            LinkTarget::Web("https://example.com".into()),
        ),
        (
            "[web](https:example.com)",
            LinkTarget::Web("https:example.com".into()),
        ),
        (
            "[web](https:/example.com)",
            LinkTarget::Web("https:/example.com".into()),
        ),
    ] {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = requests.clone();
        let activations = Arc::new(Mutex::new(0));
        let activated = activations.clone();
        let mut window = app.open_window(|_, cx| {
            let mut view = MarkdownView::new(source, false, |_| {}, cx).natural_height();
            view.set_context_handler(
                move |target, event, _, _| received.lock().unwrap().push((target, event.position)),
                cx,
            );
            view.set_target_handler(move |_| *activated.lock().unwrap() += 1, cx);
            view
        });
        window.draw();
        window.update(|view, _, cx| view.text.update(cx, |text, cx| text.select_all(cx)));
        window.draw();
        let before = window.read(|view, cx| view.text.read(cx).selected_text());
        let origin = window.update(|_, window, cx| window.focused(cx));
        let position = point(px(10.), px(10.));
        window.simulate_mouse_down(position, MouseButton::Right);
        window.simulate_mouse_up(position, MouseButton::Right);
        assert_eq!(
            requests.lock().unwrap().as_slice(),
            &[(expected.clone(), position)]
        );
        assert_eq!(*activations.lock().unwrap(), 0);
        window.update(|view, window, cx| {
            assert_eq!(window.focused(cx), origin);
            assert_eq!(view.text.read(cx).selected_text(), before);
            assert_eq!(view.source(), source);
        });
        if cfg!(target_os = "macos") {
            let modifiers = gpui_kit::Modifiers {
                control: true,
                ..Default::default()
            };
            window.simulate_event(gpui_kit::MouseDownEvent {
                position,
                button: MouseButton::Left,
                modifiers,
                click_count: 1,
                first_mouse: false,
            });
            window.simulate_event(gpui_kit::MouseUpEvent {
                position,
                button: MouseButton::Left,
                modifiers,
                click_count: 1,
            });
            assert_eq!(
                requests.lock().unwrap().as_slice(),
                &[(expected.clone(), position), (expected, position)]
            );
            assert_eq!(*activations.lock().unwrap(), 0);
            window.update(|view, window, cx| {
                assert_eq!(window.focused(cx), origin);
                assert_eq!(view.text.read(cx).selected_text(), before);
            });
        }
    }
}

#[test]
fn inline_error_mode_survives_presentation_decoration_and_copies_original_offsets() {
    struct InlineHost {
        text: Entity<TextViewState>,
        presentation: Entity<MarkdownPresentation>,
    }
    impl Render for InlineHost {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            MarkdownPresentation::decorate(&self.presentation, 12., false, window, cx)
        }
    }
    let source = "# **failed**\n\n> retry src/a.rs:42\n- item\n&amp;";
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let mut window = app.open_window(|_, cx| {
        let text = cx.new(|cx| {
            TextViewState::markdown_with_extensions(source, extensions().inline_only(true), cx)
        });
        let presentation = cx.new(|cx| {
            let mut presentation = MarkdownPresentation::new(text.clone(), false, |_| {}, cx);
            presentation.set_inline_only(true, cx);
            presentation
        });
        InlineHost { text, presentation }
    });
    window.draw();
    window.read(|host, cx| {
        let text = host.text.read(cx);
        assert!(
            text.rendered_text()
                .as_str()
                .starts_with("# failed\n\n> retry src/a.rs:42\n- item\n&")
        );
        let rendered = text.rendered_text().as_str().to_owned();
        let at = rendered.find("src/a.rs").unwrap();
        assert_eq!(
            &source[text.source_range_for_rendered_range(at..at + 8).unwrap()],
            "src/a.rs"
        );
    });
}

#[test]
fn explicit_web_destinations_follow_whatwg_url_parsing_and_keep_written_spelling() {
    for written in [
        "https:example.com",
        "https:/example.com",
        "http:example.com",
        "HTTPS://Example.COM/a%20b?q=%5C#fragment",
        "https:///example.com",
        "https://[::1]:443/a",
        "https://example.com\\path",
        " https://example.com ",
        "https://exa\tmple.com",
    ] {
        assert_eq!(
            LinkTarget::explicit(written),
            Some(LinkTarget::Web(written.into())),
            "{written:?}"
        );
    }
    for written in [
        "https:",
        "https://",
        "https://exa mple.com",
        "https://%zz",
        "https://[::1",
        "https://example.com:99999",
        "https://256.1.1.1",
        "https://user@",
        "mailto:person@example.com",
        "ftp://example.com",
        "javascript:alert(1)",
    ] {
        assert_eq!(LinkTarget::explicit(written), None, "{written:?}");
    }
}

#[test]
fn typed_targets_decode_explicit_once_but_never_written_paths() {
    assert_eq!(
        link_target(&InlineLinkTarget::Explicit("docs/a%2520b.md".into())),
        Some(LinkTarget::DocumentPath("docs/a%20b.md".into()))
    );
    assert_eq!(
        link_target(&InlineLinkTarget::WrittenPath {
            path: "/tmp/a%20b.md".into(),
            button: false
        }),
        Some(LinkTarget::WrittenPath("/tmp/a%20b.md".into()))
    );
    assert_eq!(
        link_target(&InlineLinkTarget::Explicit("javascript:alert(1)".into())),
        None
    );
}

#[test]
fn actual_renderer_skips_encoded_nontargets_anchors_images_svg_and_block_code() {
    struct Probe {
        text: Entity<TextViewState>,
        paths: Arc<Mutex<std::collections::BTreeSet<String>>>,
    }
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let paths = self.paths.clone();
            TextView::new(&self.text).markdown_extensions(extensions().inline_links(move |leaf| {
                let links = file_reference::inline_links(leaf);
                paths
                    .lock()
                    .unwrap()
                    .extend(links.iter().map(|link| link.path.to_string()));
                links
            }))
        }
    }
    let source = "src/a%20b.rs\n\n`src/raw.rs:42`\n\n[explicit](src/dest.rs)\n\nSee <a href='src/attr.rs'>src/anchor.rs</a> and <svg>src/svg.rs</svg>.\n\n![src/alt.rs](src/image.rs)\n\n```txt\nsrc/fenced.rs\n```\n\n    src/indented.rs\n\n&LT;src/entity.rs&GT;\n\nsrc/final.rs";
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let paths = Arc::new(Mutex::new(std::collections::BTreeSet::new()));
    let captured = paths.clone();
    let mut window = app.open_window(|_, cx| Probe {
        text: cx.new(|cx| TextViewState::markdown_with_extensions(source, extensions(), cx)),
        paths,
    });
    window.draw();
    assert_eq!(
        *captured.lock().unwrap(),
        std::collections::BTreeSet::from(["src/raw.rs".into(), "src/final.rs".into()])
    );
    window.read(|host, cx| {
        assert!(
            host.text
                .read(cx)
                .rendered_text()
                .as_str()
                .contains("src/raw.rs:42")
        );
    });
}

#[test]
fn link_decoration_does_not_change_paragraph_or_inline_code_wrapping() {
    struct WrapHost {
        text: Entity<TextViewState>,
        linked: bool,
        size: Arc<Mutex<Option<gpui_kit::Size<gpui_kit::Pixels>>>>,
    }
    impl Render for WrapHost {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let size = self.size.clone();
            let configured = if self.linked {
                extensions()
            } else {
                MarkdownExtensions::default()
                    .soft_breaks(true)
                    .math(false)
                    .skip_empty_table_header(true)
            };
            div()
                .w(px(140.))
                .on_prepaint(move |bounds, _, _| *size.lock().unwrap() = Some(bounds.size))
                .child(
                    TextView::new(&self.text)
                        .style(text_style(false, window))
                        .markdown_extensions(configured),
                )
        }
    }
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let size = Arc::new(Mutex::new(None));
    let captured = size.clone();
    let mut window = app.open_window(|_, cx| WrapHost {
        text: cx.new(|cx| {
            TextViewState::markdown_with_extensions(
                "See src/a.rs:42 and `~/Documents/a & b.txt:21–31` then &amp; more prose to wrap.",
                extensions(),
                cx,
            )
        }),
        linked: false,
        size: captured,
    });
    window.draw();
    let original = size.lock().unwrap().unwrap();
    assert!(original.height > px(40.));
    let before = window.read(|host, cx| host.text.read(cx).rendered_text().as_str().to_owned());
    window.update(|host, _, cx| {
        host.linked = true;
        cx.notify();
    });
    window.draw();
    assert_eq!(size.lock().unwrap().unwrap(), original);
    window.read(|host, cx| assert_eq!(host.text.read(cx).rendered_text().as_str(), before));
}

#[test]
fn generated_paths_have_native_tab_enter_and_remote_space_activation() {
    for (source, remote, key, expected) in [
        ("src/a.rs", false, "enter", 1),
        ("`src/a.rs`", false, "enter", 1),
        ("`src/a.rs`", false, "space", 0),
        ("`src/a.rs`", true, "space", 1),
        ("[file](docs/a%2520b.md)", false, "enter", 1),
        ("[file](docs/a%2520b.md)", false, "space", 0),
    ] {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = requests.clone();
        let mut window = app.open_window(|window, cx| {
            window.activate_window();
            let view = cx.new(|cx| {
                let mut view = MarkdownView::new(source, false, |_| {}, cx).natural_height();
                view.set_target_handler(move |target| received.lock().unwrap().push(target), cx);
                view.presentation.update(cx, |presentation, cx| {
                    presentation.set_copy_paths(remote, cx)
                });
                view
            });
            gpui_kit::base::Root::new(view, window, cx)
        });
        window.draw();
        let handle = app.windows()[0];
        app.update(|cx| {
            cx.update_window(handle, |_, window, cx| window.press("tab", cx))
                .unwrap();
        });
        app.run_until_parked();
        window.draw();
        window.update(|_, window, cx| assert!(window.focused(cx).is_some()));
        let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
        window.simulate_event(gpui_kit::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        window.simulate_event(gpui_kit::KeyUpEvent { keystroke });
        assert_eq!(
            requests.lock().unwrap().len(),
            expected,
            "source={source}, remote={remote}, {key}"
        );
        if expected != 0 {
            assert_eq!(
                requests.lock().unwrap().as_slice(),
                &[if source.starts_with('[') {
                    LinkTarget::DocumentPath("docs/a%20b.md".into())
                } else {
                    LinkTarget::WrittenPath("src/a.rs".into())
                }]
            );
        }
    }
}

#[test]
fn actual_path_activation_keeps_source_selection_and_entity_copy_exact() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let source = "src/a.rs:42 and &amp; trailing prose that wraps across lines";
    let mut window = app.open_window(|_, cx| {
        let mut view = MarkdownView::new(source, false, |_| {}, cx).natural_height();
        view.set_target_handler(move |target| captured.lock().unwrap().push(target), cx);
        view
    });
    window.simulate_resize(gpui_kit::size(px(140.), px(400.)));
    window.draw();
    window.simulate_click(point(px(10.), px(10.)), MouseButton::Left);
    assert_eq!(
        requests.lock().unwrap().as_slice(),
        &[LinkTarget::WrittenPath("src/a.rs".into())]
    );
    window.update(|view, _, cx| {
        assert_eq!(view.source(), source);
        view.text.update(cx, |text, cx| text.select_all(cx));
        assert_eq!(
            view.text.read(cx).selected_text().trim_end_matches('\n'),
            "src/a.rs:42 and & trailing prose that wraps across lines"
        );
        assert_eq!(
            view.text.read(cx).selected_source_range(),
            Some(0..source.len())
        );
    });
    window.draw();
    window.simulate_click(point(px(10.), px(10.)), MouseButton::Left);
    assert_eq!(
        requests.lock().unwrap().len(),
        1,
        "selected-text click must not activate"
    );
}
