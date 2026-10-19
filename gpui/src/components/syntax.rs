//! Ayu token colors from the same Shiki themes used by production, applied to Syntect grammars.
use std::{
    ops::Range,
    sync::{Arc, OnceLock},
};

use gpui_kit::{HighlightStyle, UnderlineStyle, base::text::CodeBlock, px, rgba};
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
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn color(hex: &str) -> Option<Color> {
    let hex = hex.strip_prefix('#')?;
    if !hex.is_ascii() || (hex.len() != 6 && hex.len() != 8) {
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

/// Production fence aliases, independent of the available grammar engine.
pub fn normalize_language(language: &str) -> String {
    let token = language
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match token.as_str() {
        "console" | "shell-session" | "sh" | "shell" | "zsh" => "bash",
        "patch" => "diff",
        "jsx" => "tsx",
        "js" => "javascript",
        "ts" => "typescript",
        "py" => "python",
        "rs" => "rust",
        "md" => "markdown",
        "yml" => "yaml",
        _ => return token,
    }
    .to_owned()
}

/// Display-name mapping from `src/lib/file-preview.ts`, not Syntect's broader extension set.
/// `None` means unhighlighted (including supported plain-text previews and non-text files).
pub fn language_for_filename(filename: &str) -> Option<&'static str> {
    let name = filename.to_ascii_lowercase();
    match name.as_str() {
        "dockerfile" => return Some("bash"),
        "readme" => return Some("markdown"),
        "gemfile" | "license" | "makefile" | ".editorconfig" | ".env" | ".gitattributes"
        | ".gitignore" | ".npmrc" => return None,
        _ => {}
    }
    let (base, ext) = name.rsplit_once('.')?;
    if base.is_empty() {
        return None;
    }
    Some(match ext {
        "bash" | "sh" | "zsh" => "bash",
        "cjs" | "js" | "mjs" => "javascript",
        "css" => "css",
        "diff" | "patch" => "diff",
        "go" => "go",
        "htm" | "html" => "html",
        "json" | "jsonl" => "json",
        "jsx" | "tsx" => "tsx",
        "markdown" | "md" | "mdx" => "markdown",
        "py" => "python",
        "rs" => "rust",
        "toml" => "toml",
        "ts" => "typescript",
        "vue" => "vue",
        "yaml" | "yml" => "yaml",
        _ => return None,
    })
}

fn highlight_style(style: syntect::highlighting::Style) -> HighlightStyle {
    let c = style.foreground;
    let foreground = rgba(u32::from_be_bytes([c.r, c.g, c.b, c.a])).into();
    HighlightStyle {
        color: Some(foreground),
        font_weight: style
            .font_style
            .contains(FontStyle::BOLD)
            .then_some(gpui_kit::FontWeight::BOLD),
        font_style: style
            .font_style
            .contains(FontStyle::ITALIC)
            .then_some(gpui_kit::FontStyle::Italic),
        underline: style
            .font_style
            .contains(FontStyle::UNDERLINE)
            .then_some(UnderlineStyle {
                thickness: px(1.),
                color: Some(foreground),
                wavy: false,
            }),
        ..Default::default()
    }
}

pub fn highlight(block: &CodeBlock, dark: bool) -> Vec<(Range<usize>, HighlightStyle)> {
    let Some(language) = block.lang() else {
        return Vec::new();
    };
    let language = normalize_language(&language);
    let token = language.as_str();
    let set = syntaxes();
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
                spans.push((offset..end, highlight_style(style)));
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
    fn bundled_grammar_inventory() {
        for (token, scope) in [
            ("bash", "source.shell.bash"),
            ("css", "source.css"),
            ("diff", "source.diff"),
            ("go", "source.go"),
            ("html", "text.html.basic"),
            ("javascript", "source.js"),
            ("json", "source.json"),
            ("markdown", "text.html.markdown"),
            ("python", "source.python"),
            ("rust", "source.rust"),
            ("toml", "source.toml"),
            ("typescript", "source.ts"),
            ("tsx", "source.tsx"),
            ("vue", "text.html.vue"),
            ("yaml", "source.yaml"),
        ] {
            let grammar = syntaxes().find_syntax_by_token(token).expect(token);
            assert!(
                grammar.scope.to_string().starts_with(scope),
                "{token}: {}",
                grammar.scope
            );
        }
    }

    #[test]
    fn every_supported_production_grammar_highlights_both_schemes() {
        for (language, source) in [
            ("bash", "echo \"café 🦀\" # comment\n"),
            ("css", "/* café 🦀 */\na { color: red; }\n"),
            ("diff", "--- a\n+++ b\n-café\n+🦀\n"),
            ("go", "package main\n// café 🦀\nfunc main() {}\n"),
            ("html", "<!-- café 🦀 -->\n<div>hi</div>\n"),
            ("javascript", "// café 🦀\nconst value = 1;\n"),
            ("json", "{\"café\": \"🦀\"}\n"),
            ("markdown", "# café 🦀\n**bold**\n"),
            ("python", "# café 🦀\ndef main(): return 1\n"),
            ("rust", "// café 🦀\nfn main() {}\n"),
            ("yaml", "# café 🦀\nkey: true\n"),
            (
                "toml",
                "# café 🦀\n[package]\nname = \"tau\"\nenabled = true\n",
            ),
            (
                "typescript",
                "// café 🦀\ninterface Item { name: string }\nconst item: Item = { name: 'tau' };\n",
            ),
            (
                "tsx",
                "// café 🦀\nconst view = <div title=\"tau\">{value}</div>;\n",
            ),
            (
                "vue",
                "<template><div v-if=\"ready\">café 🦀</div></template>\n<script lang=\"ts\">const ready: boolean = true;</script>\n<style>div { color: red; }</style>\n",
            ),
        ] {
            for dark in [false, true] {
                let spans = highlight(&CodeBlock::from_code(source, Some(language)), dark);
                assert!(spans.len() > 1, "{language}, dark={dark}");
                assert!(
                    spans
                        .iter()
                        .all(|(range, _)| source.get(range.clone()).is_some())
                );
                assert!(
                    spans.iter().any(|(_, style)| style.color
                        != Some(
                            highlight_style(syntect::highlighting::Style {
                                foreground: themes(dark).settings.foreground.unwrap(),
                                background: themes(dark).settings.background.unwrap(),
                                font_style: FontStyle::empty(),
                            })
                            .color
                            .unwrap()
                        )),
                    "{language} has token colors"
                );
            }
        }
    }

    #[test]
    fn rgba_and_all_font_flags_survive_mapping() {
        for alpha in [0, 128, 255] {
            let mapped = highlight_style(syntect::highlighting::Style {
                foreground: Color {
                    r: 17,
                    g: 83,
                    b: 197,
                    a: alpha,
                },
                background: Color {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
                font_style: FontStyle::BOLD | FontStyle::ITALIC | FontStyle::UNDERLINE,
            });
            assert_eq!(
                mapped.color,
                Some(rgba(u32::from_be_bytes([17, 83, 197, alpha])).into())
            );
            assert_eq!(mapped.color.unwrap().a, f32::from(alpha) / 255.);
            assert_eq!(mapped.font_weight, Some(gpui_kit::FontWeight::BOLD));
            assert_eq!(mapped.font_style, Some(gpui_kit::FontStyle::Italic));
            let underline = mapped.underline.unwrap();
            assert_eq!(underline.color, mapped.color);
            assert_eq!(underline.thickness, px(1.));
            assert!(!underline.wavy);
        }
        let plain = highlight_style(syntect::highlighting::Style {
            foreground: color("#123456").unwrap(),
            background: color("#000000").unwrap(),
            font_style: FontStyle::empty(),
        });
        assert!(plain.underline.is_none());
        assert!(plain.font_weight.is_none());
        assert!(plain.font_style.is_none());
    }

    #[test]
    fn aliases_use_real_grammars_in_both_schemes() {
        for (alias, canonical, source) in [
            ("CONSOLE extra", "bash", "echo \"café 🦀\" # comment\n"),
            ("shell-session", "bash", "echo hi\n"),
            ("patch", "diff", "--- a\n+++ b\n-old\n+new\n"),
            ("jsx", "tsx", "const view = <div>café 🦀</div>;\n"),
            ("ts", "typescript", "const café: string = \"🦀\";\n"),
            ("js", "javascript", "const café = \"🦀\";\n"),
            ("sh", "bash", "echo hi\n"),
            ("shell", "bash", "echo hi\n"),
            ("zsh", "bash", "echo hi\n"),
            ("py", "python", "print('café 🦀')\n"),
            ("rs", "rust", "fn main() {}\n"),
            ("md", "markdown", "# café 🦀\n"),
            ("yml", "yaml", "key: true\n"),
        ] {
            for dark in [false, true] {
                let spans = highlight(&CodeBlock::from_code(source, Some(alias)), dark);
                assert!(!spans.is_empty(), "{alias}");
                assert_eq!(
                    spans,
                    highlight(&CodeBlock::from_code(source, Some(canonical)), dark)
                );
                assert!(
                    spans
                        .iter()
                        .all(|(range, _)| source.get(range.clone()).is_some())
                );
                assert!(
                    spans
                        .windows(2)
                        .all(|pair| pair[0].0.end <= pair[1].0.start)
                );
            }
        }
        assert_eq!(normalize_language("jsx"), "tsx");
        assert!(!Arc::ptr_eq(
            &code_block_highlighter(true),
            &code_block_highlighter(false)
        ));
    }

    #[test]
    fn filenames_follow_production_not_syntect_extensions() {
        for (name, expected) in [
            ("Dockerfile", Some("bash")),
            ("README", Some("markdown")),
            ("a.JSX", Some("tsx")),
            ("a.TS", Some("typescript")),
            ("a.JSONL", Some("json")),
            ("a.MDX", Some("markdown")),
            ("a.CJS", Some("javascript")),
            ("a.MJS", Some("javascript")),
            ("a.HTM", Some("html")),
            ("a.PATCH", Some("diff")),
            ("a.ZSH", Some("bash")),
            ("a.YML", Some("yaml")),
            ("a.VUE", Some("vue")),
            ("a.TOML", Some("toml")),
            (".env", None),
            ("Makefile", None),
            ("a.sql", None),
            ("a.xml", None),
            ("a.ini", None),
            ("a.csv", None),
            ("a.txt", None),
            ("a.properties", None),
            ("a.log", None),
            ("a.rb", None),
            (".rs", None),
            ("no-extension", None),
        ] {
            assert_eq!(language_for_filename(name), expected, "{name}");
        }
    }

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
        for language in [
            "bash",
            "css",
            "diff",
            "go",
            "html",
            "javascript",
            "json",
            "markdown",
            "python",
            "rust",
            "toml",
            "typescript",
            "tsx",
            "vue",
            "yaml",
        ] {
            for dark in [false, true] {
                assert!(highlight(&CodeBlock::from_code("", Some(language)), dark).is_empty());
                // Unclosed strings/tags and CRLF must remain safe during streaming.
                let source = "café 🦀 = \"unterminated\r\n<broken { [\n";
                let spans = highlight(&CodeBlock::from_code(source, Some(language)), dark);
                assert!(
                    spans
                        .iter()
                        .all(|(range, _)| source.get(range.clone()).is_some())
                );
                assert!(
                    spans
                        .windows(2)
                        .all(|pair| pair[0].0.end <= pair[1].0.start)
                );
            }
        }
        assert!(highlight(&CodeBlock::from_code("café 🦀", None::<&str>), true).is_empty());
        for invalid in ["", "   ", "🦀", "not-a-real-language"] {
            assert!(highlight(&CodeBlock::from_code("café 🦀", Some(invalid)), false).is_empty());
        }
        for invalid in ["#🦀ab", "#12345", "#gggggg", "123456"] {
            assert!(color(invalid).is_none());
        }
        assert!(
            !highlight(
                &CodeBlock::from_code("fn incomplete(\"🦀", Some("rust")),
                false
            )
            .is_empty()
        );
    }
}
