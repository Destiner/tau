#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewKind {
    Text,
    Image,
    Pdf,
    Video,
    Audio,
    Unsupported,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewType {
    pub kind: PreviewKind,
    pub language: Option<&'static str>,
    pub markdown: bool,
}
pub fn classify(filename: &str) -> PreviewType {
    let name = filename.to_ascii_lowercase();
    let ext = name
        .rsplit_once('.')
        .filter(|(base, _)| !base.is_empty())
        .map_or("", |(_, ext)| ext);
    let markdown = matches!(name.as_str(), "readme") || matches!(ext, "md" | "markdown");
    let language = match name.as_str() {
        "dockerfile" => Some("bash"),
        "readme" => Some("markdown"),
        _ => match ext {
            "bash" | "sh" | "zsh" => Some("bash"),
            "js" | "cjs" | "mjs" => Some("javascript"),
            "jsx" | "tsx" => Some("tsx"),
            "ts" => Some("typescript"),
            "css" => Some("css"),
            "diff" | "patch" => Some("diff"),
            "go" => Some("go"),
            "htm" | "html" => Some("html"),
            "json" | "jsonl" => Some("json"),
            "markdown" | "md" | "mdx" => Some("markdown"),
            "py" => Some("python"),
            "rs" => Some("rust"),
            "toml" => Some("toml"),
            "vue" => Some("vue"),
            "yaml" | "yml" => Some("yaml"),
            _ => None,
        },
    };
    let kind = if language.is_some()
        || matches!(
            ext,
            "csv" | "ini" | "log" | "properties" | "sql" | "txt" | "xml"
        )
        || matches!(
            name.as_str(),
            "gemfile"
                | "license"
                | "makefile"
                | ".editorconfig"
                | ".env"
                | ".gitattributes"
                | ".gitignore"
                | ".npmrc"
        ) {
        PreviewKind::Text
    } else if matches!(
        ext,
        "avif" | "bmp" | "gif" | "ico" | "jpeg" | "jpg" | "png" | "svg" | "webp"
    ) {
        PreviewKind::Image
    } else if ext == "pdf" {
        PreviewKind::Pdf
    } else if matches!(ext, "m4v" | "mov" | "mp4" | "ogv" | "webm") {
        PreviewKind::Video
    } else if matches!(ext, "aac" | "flac" | "m4a" | "mp3" | "oga" | "ogg" | "wav") {
        PreviewKind::Audio
    } else {
        PreviewKind::Unsupported
    };
    PreviewType {
        kind,
        language,
        markdown,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn production_names_and_presentations() {
        for filename in ["guide.MD", "README", "notes.MARKDOWN"] {
            assert!(classify(filename).markdown);
        }
        assert!(!classify("component.MDX").markdown);
        for filename in ["service.TS", "Dockerfile", ".gitignore", "LICENSE", "a.xml"] {
            assert_eq!(classify(filename).kind, PreviewKind::Text);
        }
        assert_eq!(classify("diagram.svg").kind, PreviewKind::Image);
        assert_eq!(classify("manual.pdf").kind, PreviewKind::Pdf);
        assert_eq!(classify("demo.webm").kind, PreviewKind::Video);
        assert_eq!(classify("recording.flac").kind, PreviewKind::Audio);
        assert_eq!(classify("archive.zip").kind, PreviewKind::Unsupported);
        assert_eq!(classify(".md").kind, PreviewKind::Unsupported);
    }
}
