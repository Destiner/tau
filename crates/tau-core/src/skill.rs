/// The exact Pi user envelope; metadata is not inferred from ordinary prose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillEnvelope<'a> {
    pub name: &'a str,
    pub location: &'a str,
    pub content: &'a str,
    pub prompt: &'a str,
}

/// Matches Vue's parseSkillBlock without rewriting the source user message.
pub fn parse_skill_envelope(text: &str) -> Option<SkillEnvelope<'_>> {
    let rest = text.strip_prefix("<skill name=\"")?;
    let (name, rest) = rest.split_once("\" location=\"")?;
    let (location, rest) = rest.split_once("\">\n")?;
    if name.is_empty() || location.is_empty() || name.contains('"') || location.contains('"') {
        return None;
    }
    // Vue's non-greedy body can backtrack past a closing tag inside instructions.
    for (end, _) in rest.match_indices("\n</skill>") {
        let content = &rest[..end];
        let trailing = &rest[end + "\n</skill>".len()..];
        let prompt = if matches!(
            trailing,
            "" | "\n" | "\r" | "\r\n" | "\u{2028}" | "\u{2029}"
        ) {
            ""
        } else if let Some(prompt) = trailing.strip_prefix("\n\n").filter(|s| !s.is_empty()) {
            prompt.trim_matches(|c| {
                matches!(c,
                    '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}' |
                    '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
                    '\u{205f}' | '\u{3000}' | '\u{feff}'
                )
            })
        } else {
            continue;
        };
        return Some(SkillEnvelope {
            name,
            location,
            content,
            prompt,
        });
    }
    None
}

/// Presentation only: never use this value as Pi's session name.
pub fn first_message_title(text: &str) -> String {
    parse_skill_envelope(text)
        .map(|skill| format!("/skill:{}", skill.name))
        .unwrap_or_else(|| text.to_string())
}
