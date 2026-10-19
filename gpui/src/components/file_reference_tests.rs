use super::*;
#[test]
fn production_recognition_table() {
    for path in [
        "/Users/tim/notes.md",
        "./src/App.vue",
        "../tau/README.md",
        "../notes",
        "./build",
        "~/.config/pi/settings.json",
        "src/lib/markdown.ts",
        "~/Documents/file with spaces & (parens).txt",
        "src/components/",
        "reports/148/",
        "148/report.txt",
        "/README.md",
        "/tmp/",
        "/etc/hosts",
        "/Users/tim/orgs/billing",
    ] {
        assert_eq!(parse_file_reference(path).unwrap().path, path, "{path}");
    }
    for path in [
        "src/App.vue:abc",
        "148/",
        "build/",
        "and/or",
        "24/7",
        "TypeScript/JavaScript",
        "2026/08/14",
        "/pre",
        "/usage",
        "~/Documents",
        "/ expanded",
        "/Users/tim /notes.md",
        "/users/internal/orgs/:orgId/billing",
        "/billing/{events,suspend,resume}",
        "/users/[userId]/settings",
        "/files/*path",
        "https://example.com/docs/guide.md",
        "//shared/report.md",
        "/",
        "~/",
        "README.md",
    ] {
        assert!(parse_file_reference(path).is_none(), "{path}");
    }
    for marker in [":42", ":42:10", ":21-31", ":21–31"] {
        let written = format!("src/App.vue{marker}");
        assert_eq!(
            parse_file_reference(&written),
            Some(FileReference {
                text: written.clone(),
                path: "src/App.vue".into()
            })
        );
        let prose = format!("See {written}, then src/App.vue.");
        let found = references_in_text(&prose);
        assert_eq!(found.len(), 2);
        assert_eq!(&prose[found[0].range.clone()], "src/App.vue");
    }
}
#[test]
fn explicit_destinations_decode_once_and_reject_schemes_controls_and_foreign_hosts() {
    for (input, path) in [
        ("README", "README"),
        ("docs/My%20File.md", "docs/My File.md"),
        ("literal%2520name", "literal%20name"),
        ("file:///tmp/My%20File.txt", "/tmp/My File.txt"),
        ("file://localhost/tmp/a", "/tmp/a"),
        ("src/My%20App.vue:42:10", "src/My App.vue"),
        ("docs/My%20File.md:21%E2%80%9331", "docs/My File.md"),
    ] {
        assert_eq!(
            parse_markdown_file_destination(input).as_deref(),
            Some(path),
            "{input}"
        );
    }
    for input in [
        "file://server/tmp/a",
        "mailto:test@example.com",
        "%zz",
        "bad%0Aname",
        "#section",
        "https://example.com",
        "javascript:alert(1)",
        "bad%FFname",
    ] {
        assert!(parse_markdown_file_destination(input).is_none(), "{input}");
    }
}
#[test]
fn encoded_markup_routes_and_standalone_spaces() {
    for input in [
        "&lt;/pre&gt;",
        "&lt;/nested/tag&gt;",
        ":orgId/billing.json",
        "/billing/{events,suspend,resume}",
        "/users/[userId]/settings",
        "/files/*path",
        "/ expanded ",
        "148/",
    ] {
        assert!(references_in_encoded_text(input).is_empty(), "{input}");
    }
    for (encoded, path, marker) in [
        (
            "~/Documents/a &amp; b.txt",
            "~/Documents/a & b.txt",
            ":21–31",
        ),
        ("/tmp/a &#38; b.txt", "/tmp/a & b.txt", ":42:10"),
        ("/Users/tim/My File.md", "/Users/tim/My File.md", ":21-31"),
    ] {
        let run = format!(" {encoded}{marker} ");
        let spans = references_in_encoded_text(&run);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].path, path);
        assert_eq!(&run[spans[0].range.clone()], encoded);
    }
    let run = "é See src/App.vue:21–31, then ./notes.txt.";
    let spans = references_in_text(run);
    assert_eq!(spans.len(), 2);
    assert_eq!(&run[spans[0].range.clone()], "src/App.vue");
}
#[test]
fn current_production_file_url_special_paths_and_decode_controls() {
    // Expectations checked against the current markdown.ts export and JS URL.
    for (input, expected) in [
        ("file:relative", Some("/relative")),
        ("file:", Some("/")),
        ("file: foo ", Some("/ foo")),
        ("file:///a/%2e%2e/b", Some("/b")),
        ("file:///a/.%2E/b", Some("/b")),
        ("file:///a//../b", Some("/a/b")),
        ("file:///C|/a/../b", Some("/C:/b")),
        ("file:///C:/../b", Some("/C:/b")),
        ("file://C:/a", Some("/C:/a")),
        ("file://local%68ost/a", Some("/a")),
        ("file:////tmp/a", Some("//tmp/a")),
        ("file:\\tmp\\a", Some("/tmp/a")),
        ("file:///tmp/a%250A.md", Some("/tmp/a%0A.md")),
        ("file:///tmp/a%0A.md", None),
        ("file:///tmp/a%7F.md", None),
        ("file://localhost:80/a", None),
        ("file://user@localhost/a", None),
        ("file:///tmp/a%FF.md", None),
        ("FILE:///tmp/a", None),
        ("docs/a%E2%80%A8b.md:42", Some("docs/a\u{2028}b.md:42")),
    ] {
        assert_eq!(
            parse_markdown_file_destination(input).as_deref(),
            expected,
            "{input}"
        );
    }
}

#[test]
fn production_percent_entity_whitespace_and_filename_edges() {
    for input in [
        "see src/a%20b.rs now",
        "src&#47;a.rs",
        "&lt;src/a.rs&gt;",
        "src/a.abcdefghijk",
        "src/a.rs:abc",
    ] {
        assert!(references_in_encoded_text(input).is_empty(), "{input}");
    }
    for (input, path) in [
        ("/tmp/a%0A.md", "/tmp/a%0A.md"),
        ("src/.env", "src/.env"),
        ("src/a.abcdefghij", "src/a.abcdefghij"),
        ("src/a.rs:42–50", "src/a.rs"),
        ("\u{feff}/tmp/a b.md\u{feff}", "/tmp/a b.md"),
    ] {
        let spans = references_in_encoded_text(input);
        assert_eq!(spans.len(), 1, "{input}");
        assert_eq!(spans[0].path, path, "{input}");
        assert_eq!(&input[spans[0].range.clone()], path);
    }
    assert!(parse_file_reference("/tmp/\u{feff}a.md").is_none());
    assert!(parse_file_reference("/tmp/\u{0085}a.md").is_some());
    assert!(parse_file_reference("/tmp/\u{180e}a.md").is_some());
}

#[test]
fn transcript_resolution_is_not_preview_normalization() {
    let base = "/work/project\\root";
    for (written, expected) in [
        ("a\\b.txt", "/work/project\\root/a\\b.txt"),
        ("./a%5Cb.txt", "/work/project\\root/a%5Cb.txt"),
        ("/absolute\\root/a\\b.txt", "/absolute\\root/a\\b.txt"),
        ("src/../a%20b.txt", "/work/project\\root/a%20b.txt"),
    ] {
        assert_eq!(resolve_file_path(base, written, None), expected);
    }
    let decoded = parse_markdown_file_destination("a%5Cb.txt").unwrap();
    assert_eq!(decoded, "a\\b.txt");
    assert_eq!(
        resolve_file_path(base, &decoded, None),
        "/work/project\\root/a\\b.txt"
    );
    assert_eq!(
        parse_markdown_file_destination("a%255Cb.txt").as_deref(),
        Some("a%5Cb.txt")
    );
    assert_eq!(
        resolve_file_path(base, "~/a\\b%20.txt", Some("/home/user\\root")),
        "/home/user\\root/a\\b%20.txt"
    );
    assert_eq!(
        resolve_file_path(base, "~/a\\b%20.txt", None),
        "~/a\\b%20.txt"
    );
    assert_eq!(
        resolve_file_path("/work/tau", "../../../etc/hosts", None),
        "/etc/hosts"
    );
    assert_eq!(
        resolve_file_path("/work/tau", "~/notes.md", None),
        "~/notes.md"
    );
    assert_eq!(
        resolve_file_path("/work/tau", "~/notes.md", Some("/Users/tim")),
        "/Users/tim/notes.md"
    );
    assert_eq!(
        resolve_file_path("/work/tau/", "docs//pi.md", None),
        "/work/tau/docs/pi.md"
    );
}
