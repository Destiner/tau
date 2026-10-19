use super::*;
use gpui_kit::{AppContext, gpui, test::TestWindowExt};

#[gpui::test]
fn real_preview_loading_error_close_and_exact_code_projection(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| crate::components::init(cx, false));
    let mut retained = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let viewer = cx.new(|cx| {
            FileViewer::new(
                "example.rs",
                "src",
                FileContent::Loading,
                None,
                false,
                window,
                cx,
            )
        });
        retained = Some(viewer.clone());
        gpui_kit::base::Root::new(viewer, window, cx)
    });
    let viewer = retained.unwrap();
    window.update(|window, cx| {
        let _ = window.draw(cx);
        viewer.update(cx, |viewer, cx| viewer.set_loading_visible(true, cx));
        let _ = window.draw(cx);
        assert!(window.find("close").visible());
        for text in [
            "fn main() {}\n\n\n",
            "fn main() {}",
            "// ~~~\n~~~\n~~~~\n",
            "// CRLF\r\n",
            "",
        ] {
            viewer.update(cx, |viewer, cx| {
                viewer.set_content(FileContent::Text(text.into()), cx)
            });
            let _ = window.draw(cx);
            assert_eq!(viewer.read(cx).rendered_text(cx).as_deref(), Some(text));
        }
        viewer.update(cx, |viewer, cx| {
            viewer.set_content(FileContent::Error("Couldn’t preview this file.".into()), cx)
        });
        let _ = window.draw(cx);
        assert!(window.find("close").visible());
        viewer.update(cx, |viewer, cx| viewer.close(cx));
        assert!(viewer.read(cx).is_closed());
    });
}
#[test]
fn text_fences_do_not_close_on_snapshot_contents_or_erase_blank_lines() {
    let text = "```\n~~~\n~~~~\n\n\n";
    let fence = source_fence(text, "rust");
    assert_eq!(fence, format!("~~~~~rust\n{text}~~~~~"));
    assert!(highlightable(text));
    assert!(!highlightable("no newline"));
    assert!(!highlightable("CR\r\n"));
    assert!(!highlightable(&format!("{}\n", "a".repeat(20_001))));
}
#[test]
fn document_requests_reject_unsafe_targets_and_decode_explicit_links_once() {
    use super::super::markdown::LinkTarget;
    assert_eq!(
        LinkTarget::explicit("docs/My%20File.md:21%E2%80%9331"),
        Some(LinkTarget::DocumentPath("docs/My File.md".into()))
    );
    for target in [
        "javascript:alert(1)",
        "#section",
        "file://server/tmp/a",
        "%zz",
    ] {
        assert!(LinkTarget::explicit(target).is_none());
    }
}
