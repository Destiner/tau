use serde_json::Value;

pub(crate) fn valid_model(model: &Value) -> bool {
    ["provider", "id"]
        .iter()
        .all(|key| model[*key].as_str().is_some_and(|s| !s.is_empty()))
}

pub(crate) fn scope_models<'a>(models: &'a [Value], patterns: &[String]) -> Vec<&'a Value> {
    let mut scoped: Vec<&Value> = Vec::new();
    for pattern in patterns {
        let pattern = pattern.trim();
        let matches = if pattern.contains(['*', '?', '[']) {
            let pattern = without_thinking(pattern);
            if let Some(model) = exact(models, pattern) {
                vec![model]
            } else {
                models
                    .iter()
                    .filter(|m| {
                        glob(
                            pattern,
                            &format!("{}/{}", text(m, "provider"), text(m, "id")),
                        ) || glob(pattern, text(m, "id"))
                    })
                    .collect()
            }
        } else {
            reference(models, pattern).into_iter().collect()
        };
        for model in matches {
            if !scoped.iter().any(|m| std::ptr::eq(*m, model)) {
                scoped.push(model);
            }
        }
    }
    if scoped.is_empty() {
        models.iter().collect()
    } else {
        scoped
    }
}

fn text<'a>(model: &'a Value, key: &str) -> &'a str {
    model[key].as_str().unwrap_or_default()
}
fn single<'a>(mut models: impl Iterator<Item = &'a Value>) -> Option<&'a Value> {
    let first = models.next()?;
    models.next().is_none().then_some(first)
}
fn exact<'a>(models: &'a [Value], pattern: &str) -> Option<&'a Value> {
    let pattern = pattern.trim().to_lowercase();
    if pattern.is_empty() {
        return None;
    }
    let canonical: Vec<_> = models
        .iter()
        .filter(|m| format!("{}/{}", text(m, "provider"), text(m, "id")).to_lowercase() == pattern)
        .collect();
    if !canonical.is_empty() {
        return single(canonical.into_iter());
    }
    if let Some((provider, id)) = pattern.split_once('/') {
        let matched: Vec<_> = models
            .iter()
            .filter(|m| {
                text(m, "provider").to_lowercase() == provider.trim()
                    && text(m, "id").to_lowercase() == id.trim()
            })
            .collect();
        if !matched.is_empty() {
            return single(matched.into_iter());
        }
    }
    single(
        models
            .iter()
            .filter(|m| text(m, "id").to_lowercase() == pattern),
    )
}
fn alias(id: &str) -> bool {
    let dated = id
        .rsplit_once('-')
        .is_some_and(|(_, suffix)| suffix.len() == 8 && suffix.bytes().all(|c| c.is_ascii_digit()));
    id.ends_with("-latest") || !dated
}
fn reference<'a>(models: &'a [Value], pattern: &str) -> Option<&'a Value> {
    if pattern.is_empty() {
        return None;
    }
    if let Some(model) = exact(models, pattern) {
        return Some(model);
    }
    let needle = pattern.to_lowercase();
    let matched: Vec<_> = models
        .iter()
        .filter(|m| {
            text(m, "id").to_lowercase().contains(&needle)
                || text(m, "name").to_lowercase().contains(&needle)
        })
        .collect();
    let has_alias = matched.iter().any(|m| alias(text(m, "id")));
    if let Some(model) = matched
        .into_iter()
        .filter(|m| !has_alias || alias(text(m, "id")))
        .max_by_key(|m| text(m, "id"))
    {
        return Some(model);
    }
    pattern
        .rsplit_once(':')
        .and_then(|(base, _)| reference(models, base))
}
fn without_thinking(pattern: &str) -> &str {
    match pattern.rsplit_once(':') {
        Some((base, "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max")) => base,
        _ => pattern,
    }
}
fn glob(pattern: &str, value: &str) -> bool {
    fn matches(p: &[char], v: &[char]) -> bool {
        match p.first() {
            None => v.is_empty(),
            Some('*') => {
                let double = p.get(1) == Some(&'*');
                let tail = &p[if double { 2 } else { 1 }..];
                for n in 0..=v.len() {
                    if matches(tail, &v[n..]) {
                        return true;
                    }
                    if n < v.len() && !double && v[n] == '/' {
                        break;
                    }
                }
                false
            }
            Some('?') => !v.is_empty() && v[0] != '/' && matches(&p[1..], &v[1..]),
            Some('[') if p.iter().position(|c| *c == ']').is_some() => {
                let end = p.iter().position(|c| *c == ']').unwrap();
                if v.is_empty() {
                    return false;
                }
                let negated = p.get(1) == Some(&'!') || p.get(1) == Some(&'^');
                let class = &p[if negated { 2 } else { 1 }..end];
                let mut included = false;
                let mut i = 0;
                while i < class.len() {
                    if i + 2 < class.len() && class[i + 1] == '-' {
                        included |= class[i] <= v[0] && v[0] <= class[i + 2];
                        i += 3;
                    } else {
                        included |= class[i] == v[0];
                        i += 1;
                    }
                }
                included != negated && matches(&p[end + 1..], &v[1..])
            }
            Some(c) => v.first() == Some(c) && matches(&p[1..], &v[1..]),
        }
    }
    matches(
        &pattern.to_lowercase().chars().collect::<Vec<_>>(),
        &value.to_lowercase().chars().collect::<Vec<_>>(),
    )
}

/// A nonempty RPC scope is authoritative only when the entire vector is well formed.
/// Pi session scopes contain `{ model, thinkingLevel }` entries, not settings globs.
pub(crate) fn resolved_scope(data: &Value) -> Option<Vec<Value>> {
    data["models"]
        .as_array()?
        .iter()
        .map(|entry| {
            let model = if entry.get("model").is_some() {
                &entry["model"]
            } else {
                entry
            };
            valid_model(model).then(|| model.clone())
        })
        .collect()
}
