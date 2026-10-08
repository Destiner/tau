use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::{collections::HashSet, fmt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub key: String,
    pub value: String,
}

pub struct Preview<'a> {
    pub body: &'a str,
    pub fields: Vec<Field>,
}

// Vec-backed maps preserve YAML order at every level. String-only keys are
// checked after deserialize_any so numeric/bool keys cannot be coerced.
#[derive(Debug)]
enum Value {
    Primitive(String),
    Null,
    Seq(Vec<Value>),
    Map(Vec<(String, Value)>),
}
impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let serde_saphyr::Tagged(Raw(value), tag) = serde_saphyr::Tagged::<Raw>::deserialize(d)?;
        if matches!(
            tag.as_deref(),
            Some(
                "tag:yaml.org,2002:binary"
                    | "tag:yaml.org,2002:set"
                    | "tag:yaml.org,2002:timestamp"
            )
        ) {
            return Err(de::Error::custom("Unsupported frontmatter value"));
        }
        Ok(value)
    }
}
struct Raw(Value);
impl<'de> Deserialize<'de> for Raw {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Value;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("preview YAML value")
            }
            fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
                Ok(Value::Primitive(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
                Ok(Value::Primitive(v))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
                Ok(Value::Primitive(v.to_string()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
                self.visit_f64(v as f64)
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
                self.visit_f64(v as f64)
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
                let s = if v.is_nan() {
                    "NaN".into()
                } else if v == f64::INFINITY {
                    "Infinity".into()
                } else if v == f64::NEG_INFINITY {
                    "-Infinity".into()
                } else if v == 0. {
                    "0".into()
                } else if !(1e-6..1e21).contains(&v.abs()) {
                    let raw = format!("{v:e}");
                    let (mantissa, exponent) = raw.split_once('e').unwrap();
                    let exponent: i32 = exponent.parse().unwrap();
                    format!(
                        "{mantissa}e{}{exponent}",
                        if exponent >= 0 { "+" } else { "" }
                    )
                } else {
                    v.to_string()
                };
                Ok(Value::Primitive(s))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
                let mut items = Vec::new();
                while let Some(v) = a.next_element()? {
                    items.push(v);
                }
                Ok(Value::Seq(items))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
                let mut entries = Vec::new();
                // Membership is map-local; the Vec alone owns presentation order.
                let mut keys = HashSet::new();
                while let Some(serde_saphyr::Tagged(key, tag)) =
                    a.next_key::<serde_saphyr::Tagged<Key>>()?
                {
                    if matches!(
                        tag.as_deref(),
                        Some("tag:yaml.org,2002:binary" | "tag:yaml.org,2002:timestamp")
                    ) {
                        return Err(de::Error::custom("Unsupported frontmatter key"));
                    }
                    if !keys.insert(key.0.clone()) {
                        return Err(de::Error::custom("Duplicate key"));
                    }
                    entries.push((key.0, a.next_value()?));
                }
                Ok(Value::Map(entries))
            }
        }
        d.deserialize_any(V).map(Raw)
    }
}
struct Key(String);
impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct K;
        impl Visitor<'_> for K {
            type Value = Key;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("string key")
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<Key, E> {
                Ok(Key(s.into()))
            }
            fn visit_string<E: de::Error>(self, s: String) -> Result<Key, E> {
                Ok(Key(s))
            }
        }
        d.deserialize_any(K)
    }
}
fn format_value(value: &Value, depth: usize) -> Option<String> {
    if depth > 20 {
        return None;
    }
    Some(match value {
        Value::Primitive(s) => s
            .split(|c: char| (c.is_whitespace() && c != '\u{0085}') || c == '\u{feff}')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        Value::Null => "null".into(),
        Value::Seq(items) if items.is_empty() => "[]".into(),
        Value::Map(items) if items.is_empty() => "{}".into(),
        Value::Seq(items) => items
            .iter()
            .map(|v| format_value(v, depth + 1))
            .collect::<Option<Vec<_>>>()?
            .join(if items.iter().any(|v| matches!(v, Value::Map(_))) {
                "; "
            } else {
                ", "
            }),
        Value::Map(items) => items
            .iter()
            .map(|(k, v)| Some(format!("{k} · {}", format_value(v, depth + 1)?)))
            .collect::<Option<Vec<_>>>()?
            .join(", "),
    })
}
fn fence(line: &str, closing: bool) -> bool {
    let line = if let Some(line) = line.strip_suffix('\n') {
        line.strip_suffix('\r').unwrap_or(line)
    } else {
        line
    };
    let line = line.trim_end_matches([' ', '\t']);
    line == "---" || (closing && line == "...")
}
pub fn parse(source: &str) -> Preview<'_> {
    let fallback = || Preview {
        body: source,
        fields: Vec::new(),
    };
    if source.len() > 512 * 1024 {
        return fallback();
    }
    let opening_source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let Some((opening, rest)) = opening_source.split_once('\n') else {
        return fallback();
    };
    if !fence(opening.strip_suffix('\r').unwrap_or(opening), false) {
        return fallback();
    }
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if fence(line, true) {
            let yaml = &rest[..offset];
            let options = serde_saphyr::options! {
                strict_booleans: true,
                merge_keys: serde_saphyr::options::MergeKeyPolicy::AsOrdinary,
                budget: serde_saphyr::budget! {
                    max_depth: 24,
                    max_documents: 1,
                    max_nodes: 32768,
                    max_events: 65536,
                    max_total_scalar_bytes: 512 * 1024,
                    max_recorded_anchor_bytes: 512 * 1024,
                },
                alias_limits: serde_saphyr::alias_limits! {
                    max_total_replayed_events: 32768,
                    max_replay_stack_depth: 24,
                    max_alias_expansions_per_anchor: 100,
                },
            };
            let body = &rest[offset + line.len()..];
            let Ok(value) = serde_saphyr::from_str_with_options::<Value>(yaml, options) else {
                return fallback();
            };
            if matches!(value, Value::Null) && yaml.trim().is_empty() {
                return Preview {
                    body,
                    fields: vec![],
                };
            }
            let Value::Map(items) = value else {
                return fallback();
            };
            let fields = items
                .into_iter()
                .map(|(key, value)| {
                    Some(Field {
                        key,
                        value: format_value(&value, 0)?,
                    })
                })
                .collect::<Option<Vec<_>>>();
            return fields
                .map(|fields| Preview { body, fields })
                .unwrap_or_else(fallback);
        }
        offset += line.len();
    }
    fallback()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordered_production_values() {
        let p = parse(
            "---\ntitle: September release\ndate: 2026-09-26\ntags: [desktop, release]\nsummary: |\n  First line\n  second line\nauthors:\n  - name: Amina\n    role: maintainer\n  - name: Jules\n    role: reviewer\npublished: false\nempty: [[], {}]\nnothing: null\n---\n# Release\n",
        );
        assert_eq!(p.body, "# Release\n");
        assert_eq!(
            p.fields
                .iter()
                .map(|f| (f.key.as_str(), f.value.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("title", "September release"),
                ("date", "2026-09-26"),
                ("tags", "desktop, release"),
                ("summary", "First line second line"),
                (
                    "authors",
                    "name · Amina, role · maintainer; name · Jules, role · reviewer"
                ),
                ("published", "false"),
                ("empty", "[]; {}"),
                ("nothing", "null")
            ]
        );
    }
    #[test]
    fn fences_and_empty() {
        for source in ["---\n---\n# Body", "\u{feff}--- \t\r\n...\r\n# Body"] {
            let p = parse(source);
            assert_eq!(p.body, "# Body");
            assert!(p.fields.is_empty());
        }
        assert_eq!(parse("---\ntitle: Note\n...").body, "");
    }
    #[test]
    fn full_source_fallback() {
        for source in [
            "# Heading\n---\ntitle: Not metadata\n---\n",
            "---\ntitle: Unclosed\n",
            "---\ntitle: [bad\n---\n# Body\n",
            "---\ntitle: First\ntitle: Second\n---\n# Body\n",
            "---\n- not a mapping\n---\n# Body\n",
            "---\nitem: &item [*item]\n---\n# Body\n",
            "---\n1: invalid\n---\n",
            "---\nx: {true: invalid}\n---\n",
            "---\nx: {a: 1, a: 2}\n---\n",
            "---\nnull\n---\n",
            "---\nx: !!binary SGVsbG8=\n---\n",
            "---\nx: !!set {a: null}\n---\n",
            "---\nx: !!timestamp 2026-09-26\n---\n",
        ] {
            let p = parse(source);
            assert_eq!(p.body, source);
            assert!(p.fields.is_empty());
        }
    }
    #[test]
    fn primitive_strings_and_nested_order() {
        let p = parse(
            "---\ntruth: true\nyes: yes\nnumber: 12.5\nzero: -0.0\nbig: 1e21\nsmall: 1e-7\nquoted: '  a   b  '\nmap: {z: null, a: [1, false], empty: {}}\n---\n",
        );
        assert_eq!(
            p.fields
                .iter()
                .map(|f| f.value.as_str())
                .collect::<Vec<_>>(),
            vec![
                "true",
                "yes",
                "12.5",
                "0",
                "1e+21",
                "1e-7",
                "a b",
                "z · null, a · 1, false, empty · {}"
            ]
        );
        let p = parse("---\nx: {}\n---\n");
        assert_eq!(p.fields[0].value, "{}");
    }

    fn unique_key_source(count: usize) -> String {
        let mut source = String::from("---\n");
        for index in 0..count {
            use std::fmt::Write;
            writeln!(source, "k{index:05}: value").unwrap();
        }
        source.push_str("---\n# Body\r\n");
        source
    }

    #[test]
    fn large_unique_map_preserves_every_field_in_order() {
        let source = unique_key_source(16_000);
        let p = parse(&source);
        assert_eq!(p.body, "# Body\r\n");
        assert_eq!(p.fields.len(), 16_000);
        for (index, field) in p.fields.iter().enumerate() {
            assert_eq!(field.key, format!("k{index:05}"));
            assert_eq!(field.value, "value");
        }
        // Deterministic complexity evidence lives in visit_map: one HashSet
        // insertion per decoded key, with no scan of accumulated entries.
    }

    #[test]
    fn duplicate_decoded_keys_fall_back_to_exact_source() {
        for yaml in [
            "same: first\nsame: second\n",
            "outer: {same: first, same: second}\n",
            "outer: [{same: first, same: second}]\n",
            "name: first\n\"\\u006eame\": second\n",
            "outer: {name: first, \"\\u006eame\": second}\n",
            "&key same: first\n*key : second\n",
            "outer: {&key same: first, *key : second}\n",
            "original: &map {same: first, same: second}\ncopy: *map\n",
        ] {
            let source = format!("\u{feff}---\r\n{yaml}...\r\n# Body\r\n");
            let p = parse(&source);
            assert_eq!(p.body, source, "{yaml}");
            assert!(p.fields.is_empty(), "{yaml}");
        }
    }

    #[test]
    fn aliases_and_separate_maps_keep_local_membership_and_order() {
        let p = parse(
            "---\nfirst: &map {z: one, a: two}\nsecond: *map\nsequence: [*map, {z: three, a: four}]\n&key shared: original\nother: {*key : nested}\n---\nbody",
        );
        assert_eq!(p.body, "body");
        assert_eq!(
            p.fields,
            vec![
                Field {
                    key: "first".into(),
                    value: "z · one, a · two".into()
                },
                Field {
                    key: "second".into(),
                    value: "z · one, a · two".into()
                },
                Field {
                    key: "sequence".into(),
                    value: "z · one, a · two; z · three, a · four".into()
                },
                Field {
                    key: "shared".into(),
                    value: "original".into()
                },
                Field {
                    key: "other".into(),
                    value: "shared · nested".into()
                },
            ]
        );
    }

    #[test]
    #[ignore = "manual public-parse timing; no machine-speed assertion"]
    fn benchmark_unique_keys() {
        for count in [1_000, 8_000, 16_000] {
            let source = unique_key_source(count);
            let start = std::time::Instant::now();
            let p = parse(&source);
            let elapsed = start.elapsed();
            assert_eq!(p.fields.len(), count);
            assert_eq!(p.body, "# Body\r\n");
            eprintln!(
                "frontmatter parse: {count} keys, {} bytes, {elapsed:?}",
                source.len()
            );
        }
    }

    #[test]
    fn depth_boundary_and_aliases() {
        for depth in [20, 21, 1000] {
            let source = format!(
                "---\nx: {}0{}\n---\nbody",
                "[".repeat(depth),
                "]".repeat(depth)
            );
            let p = parse(&source);
            assert_eq!(p.fields.is_empty(), depth > 20);
            if depth > 20 {
                assert_eq!(p.body, source);
            }
        }
        let p = parse("---\nx: &a [one, two]\ny: *a\n---\n");
        assert_eq!(p.fields[1].value, "one, two");
    }
}
