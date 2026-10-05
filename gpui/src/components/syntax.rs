//! Ayu token colors from the same Shiki themes used by production, applied to Syntect grammars.
use std::{
    ops::Range,
    sync::{Arc, OnceLock},
};

use gpui_kit::{HighlightStyle, base::text::CodeBlock, rgb};
use serde_json::Value;
use syntect::{
    easy::HighlightLines,
    highlighting::{Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

pub type CodeHighlighter =
    Arc<dyn Fn(&CodeBlock) -> Vec<(Range<usize>, HighlightStyle)> + Send + Sync>;

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn color(hex: &str) -> Option<Color> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    Some(Color {
        r: u8::from_str_radix(&hex[0..2], 16).ok()?,
        g: u8::from_str_radix(&hex[2..4], 16).ok()?,
        b: u8::from_str_radix(&hex[4..6], 16).ok()?,
        a: if hex.len() == 8 {
            u8::from_str_radix(&hex[6..8], 16).ok()?
        } else {
            255
        },
    })
}

fn theme(source: &str) -> Theme {
    let data: Value = serde_json::from_str(source).expect("bundled Ayu theme JSON");
    let foreground = data["colors"]["editor.foreground"].as_str().and_then(color);
    let background = data["colors"]["editor.background"].as_str().and_then(color);
    let mut theme = Theme {
        name: data["name"].as_str().map(str::to_owned),
        settings: syntect::highlighting::ThemeSettings {
            foreground,
            background,
            ..Default::default()
        },
        ..Default::default()
    };
    for rule in data["tokenColors"].as_array().expect("tokenColors") {
        let settings = &rule["settings"];
        if rule.get("scope").is_none() {
            if let Some(value) = settings["foreground"].as_str().and_then(color) {
                theme.settings.foreground = Some(value);
            }
            continue;
        }
        let scopes = match &rule["scope"] {
            Value::String(scope) => scope.clone(),
            Value::Array(scopes) => scopes
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            _ => continue,
        };
        let Ok(scope) = scopes.parse::<ScopeSelectors>() else {
            continue;
        };
        let font_style = settings["fontStyle"].as_str().map(|style| {
            let mut flags = FontStyle::empty();
            for word in style.split_whitespace() {
                flags |= match word {
                    "bold" => FontStyle::BOLD,
                    "italic" => FontStyle::ITALIC,
                    "underline" => FontStyle::UNDERLINE,
                    _ => FontStyle::empty(),
                };
            }
            flags
        });
        theme.scopes.push(ThemeItem {
            scope,
            style: StyleModifier {
                foreground: settings["foreground"].as_str().and_then(color),
                background: settings["background"].as_str().and_then(color),
                font_style,
            },
        });
    }
    theme
}

fn themes(dark: bool) -> &'static Theme {
    static DARK: OnceLock<Theme> = OnceLock::new();
    static LIGHT: OnceLock<Theme> = OnceLock::new();
    if dark {
        DARK.get_or_init(|| theme(include_str!("../../assets/product-syntax/ayu-dark.json")))
    } else {
        LIGHT.get_or_init(|| theme(include_str!("../../assets/product-syntax/ayu-light.json")))
    }
}

/// Stable identity lets the text view cache highlighted blocks between renders.
pub fn code_block_highlighter(dark: bool) -> CodeHighlighter {
    static DARK: OnceLock<CodeHighlighter> = OnceLock::new();
    static LIGHT: OnceLock<CodeHighlighter> = OnceLock::new();
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
    // Syntect's bundled grammars include JS but not TypeScript. This is a grammar fallback,
    // not fixture-dependent coloring; syntax that JS cannot parse remains best-effort.
    let token = match token.as_str() {
        "ts" | "tsx" | "typescript" => "javascript",
        _ => &token,
    };
    let Some(syntax) = set
        .find_syntax_by_token(token)
        .or_else(|| set.find_syntax_by_extension(token))
    else {
        return Vec::new();
    };
    let theme = themes(dark);
    let mut parser = HighlightLines::new(syntax, theme);
    let code = block.code();
    let mut offset = 0;
    let mut spans = Vec::new();
    for line in LinesWithEndings::from(code.as_ref()) {
        let Ok(regions) = parser.highlight_line(line, set) else {
            return Vec::new();
        };
        for (style, region) in regions {
            let end = offset + region.len();
            if !region.trim().is_empty() && code.get(offset..end).is_some() {
                let foreground = style.foreground;
                let background = theme.settings.background.unwrap_or(Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                });
                let blend = |channel: u8, base: u8| -> u32 {
                    (u32::from(channel) * u32::from(foreground.a)
                        + u32::from(base) * (255 - u32::from(foreground.a)))
                        / 255
                };
                let value = blend(foreground.r, background.r) << 16
                    | blend(foreground.g, background.g) << 8
                    | blend(foreground.b, background.b);
                spans.push((
                    offset..end,
                    HighlightStyle {
                        color: Some(rgb(value).into()),
                        font_weight: style
                            .font_style
                            .contains(FontStyle::BOLD)
                            .then_some(gpui_kit::FontWeight::BOLD),
                        font_style: style
                            .font_style
                            .contains(FontStyle::ITALIC)
                            .then_some(gpui_kit::FontStyle::Italic),
                        ..Default::default()
                    },
                ));
            }
            offset = end;
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_themes_match_production_ayu_tokens() {
        for (dark, foreground, comment, string) in [
            (true, "#bfbdb6", "#5a6673", "#aad94c"),
            (false, "#5c6166", "#adaeb1", "#86b300"),
        ] {
            let source = if dark {
                include_str!("../../assets/product-syntax/ayu-dark.json")
            } else {
                include_str!("../../assets/product-syntax/ayu-light.json")
            };
            let value: Value = serde_json::from_str(source).unwrap();
            assert_eq!(value["colors"]["editor.foreground"], foreground);
            assert_eq!(themes(dark).settings.foreground, color(foreground));
            for (scope, expected) in [("comment", comment), ("string", string)] {
                let entry = value["tokenColors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|rule| {
                        rule["scope"]
                            .as_array()
                            .is_some_and(|scopes| scopes.iter().any(|item| item == scope))
                    })
                    .unwrap();
                assert_eq!(entry["settings"]["foreground"], expected);
                assert!(
                    themes(dark)
                        .scopes
                        .iter()
                        .any(|item| item.style.foreground == color(expected))
                );
            }
        }
    }

    #[test]
    fn real_grammar_ranges_are_valid_utf8_and_faults_are_safe() {
        let block = CodeBlock::from_code("let café = \"🦀\";\nfn main() {}", Some("rust"));
        for dark in [false, true] {
            let spans = highlight(&block, dark);
            assert!(spans.len() > 3);
            assert!(
                spans
                    .iter()
                    .all(|(range, _)| block.code().get(range.clone()).is_some())
            );
            assert!(
                spans
                    .iter()
                    .any(|(range, _)| range.start > "let café".len())
            );
            assert!(Arc::ptr_eq(
                &code_block_highlighter(dark),
                &code_block_highlighter(dark)
            ));
        }
        assert!(
            highlight(
                &CodeBlock::from_code("hi", Some("not-a-real-language")),
                true
            )
            .is_empty()
        );
        assert!(highlight(&CodeBlock::from_code("", Some("rust")), false).is_empty());
        assert!(
            !highlight(
                &CodeBlock::from_code("fn incomplete(\"🦀", Some("rust")),
                false
            )
            .is_empty()
        );
    }
}
