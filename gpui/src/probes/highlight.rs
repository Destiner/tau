//! Shared Syntect adapter for Base fenced code blocks (also usable by file/activity views).
use std::{
    ops::Range,
    sync::{Arc, OnceLock},
};

use gpui_kit::{HighlightStyle, base::text::CodeBlock, rgb};
use syntect::{
    easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet, util::LinesWithEndings,
};

pub type CodeHighlighter =
    Arc<dyn Fn(&CodeBlock) -> Vec<(Range<usize>, HighlightStyle)> + Send + Sync>;

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn themes() -> &'static ThemeSet {
    static SET: OnceLock<ThemeSet> = OnceLock::new();
    SET.get_or_init(ThemeSet::load_defaults)
}

/// Stable identity matters: Base caches each block's spans by highlighter Arc identity.
pub fn code_block_highlighter(dark: bool) -> CodeHighlighter {
    static LIGHT: OnceLock<CodeHighlighter> = OnceLock::new();
    static DARK: OnceLock<CodeHighlighter> = OnceLock::new();
    (if dark { &DARK } else { &LIGHT })
        .get_or_init(|| Arc::new(move |block| highlight(block, dark)))
        .clone()
}

pub fn highlight(block: &CodeBlock, dark: bool) -> Vec<(Range<usize>, HighlightStyle)> {
    let Some(language) = block.lang() else {
        return Vec::new();
    };
    let token = language
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let set = syntaxes();
    let Some(syntax) = set
        .find_syntax_by_token(&token)
        .or_else(|| set.find_syntax_by_extension(&token))
    else {
        return Vec::new();
    };
    let theme = &themes().themes[if dark {
        "base16-ocean.dark"
    } else {
        "InspiredGitHub"
    }];
    let mut parser = HighlightLines::new(syntax, theme);
    let code = block.code();
    let mut offset = 0;
    let mut spans = Vec::new();
    for line in LinesWithEndings::from(code.as_ref()) {
        match parser.highlight_line(line, set) {
            Ok(regions) => {
                for (style, region) in regions {
                    let end = offset + region.len();
                    if !region.trim().is_empty() {
                        let color = style.foreground;
                        spans.push((
                            offset..end,
                            HighlightStyle {
                                color: Some(
                                    rgb(u32::from(color.r) << 16
                                        | u32::from(color.g) << 8
                                        | u32::from(color.b))
                                    .into(),
                                ),
                                ..Default::default()
                            },
                        ));
                    }
                    offset = end;
                }
            }
            Err(_) => return Vec::new(),
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_rust_and_uses_byte_offsets_for_unicode() {
        let block = CodeBlock::from_code("let café = \"🦀\";\nfn main() {}", Some("rust"));
        for dark in [false, true] {
            let spans = highlight(&block, dark);
            assert!(spans.len() > 3);
            let code = block.code();
            assert!(
                spans
                    .iter()
                    .all(|(range, _)| code.get(range.clone()).is_some())
            );
            assert!(
                spans
                    .iter()
                    .any(|(range, _)| code.get(range.clone()).unwrap().contains("fn"))
            );
            assert!(
                spans
                    .iter()
                    .any(|(range, _)| range.start > "let café".len())
            );
            let colors: std::collections::HashSet<_> = spans
                .iter()
                .map(|(_, style)| format!("{:?}", style.color))
                .collect();
            assert!(
                colors.len() > 1,
                "syntax must produce distinct paint colors"
            );
        }
    }

    #[test]
    fn unknown_empty_and_unfinished_are_safe() {
        assert!(
            highlight(
                &CodeBlock::from_code("hi", Some("not-a-real-language")),
                false
            )
            .is_empty()
        );
        assert!(highlight(&CodeBlock::from_code("", Some("rust")), true).is_empty());
        assert!(
            !highlight(
                &CodeBlock::from_code("fn incomplete(\"🦀", Some("rust")),
                false
            )
            .is_empty()
        );
    }
}
