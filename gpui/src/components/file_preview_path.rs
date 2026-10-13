//! Lexical path context from production file-preview.ts (not filesystem canonicalization).
pub fn absolute(path: &str) -> bool {
    path.starts_with('/')
        || (path.len() >= 3
            && path.as_bytes()[0].is_ascii_alphabetic()
            && path.as_bytes()[1] == b':'
            && path.as_bytes()[2] == b'/')
}
pub fn normalize(value: &str) -> String {
    let value = value.replace('\\', "/");
    let drive = value.len() >= 2
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value.as_bytes()[1] == b':';
    let rooted = absolute(&value);
    let prefix = if drive {
        format!("{}/", &value[..2])
    } else if value.starts_with("//") {
        "//".into()
    } else if rooted {
        "/".into()
    } else {
        String::new()
    };
    let body = if drive { &value[2..] } else { &value };
    let mut parts = Vec::new();
    for part in body.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            ".." if rooted => {}
            _ => parts.push(part),
        }
    }
    let result = format!("{prefix}{}", parts.join("/"));
    if result.is_empty() {
        ".".into()
    } else {
        result
    }
}
pub fn directory_label(directory: &str, project: Option<&str>) -> String {
    let normalized = normalize(directory);
    let Some(project) = project.filter(|_| absolute(&normalized)) else {
        return normalized;
    };
    let project = normalize(project);
    let insensitive = project.len() >= 3 && project.as_bytes()[1] == b':';
    let compared = if insensitive {
        normalized.to_lowercase()
    } else {
        normalized.clone()
    };
    let root = if insensitive {
        project.to_lowercase()
    } else {
        project.clone()
    };
    if compared == root {
        return ".".into();
    }
    // Preserve Vue's root-prefix behavior, including the double slash for '/'.
    if compared.starts_with(&format!("{root}/")) {
        normalized[project.len() + 1..].into()
    } else {
        normalized
    }
}
pub fn directory_for_path(path: &str, project: Option<&str>) -> String {
    let path = normalize(path);
    let directory = match path.rfind('/') {
        None => ".",
        Some(0) => "/",
        Some(index) => &path[..index],
    };
    directory_label(directory, project)
}
pub fn resolve_context(path: &str, source: &str, project: Option<&str>) -> String {
    if absolute(path) || path == "~" || path.starts_with("~/") {
        return normalize(path);
    }
    let mut source = normalize(source);
    if let Some(project) =
        project.filter(|_| !absolute(&source) && source != "~" && !source.starts_with("~/"))
    {
        source = normalize(&format!("{project}/{source}"));
    }
    let directory = match source.rfind('/') {
        None => ".",
        Some(0) => "/",
        Some(index) => &source[..index],
    };
    normalize(&format!("{directory}/{path}"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn production_context_and_labels() {
        assert_eq!(normalize("src\\components\\..\\lib"), "src/lib");
        assert_eq!(normalize("../../fixtures/../sample"), "../../sample");
        assert_eq!(
            resolve_context(
                "../images/My File.png",
                "/work/tau/docs/guide/README.md",
                Some("/work/tau")
            ),
            "/work/tau/docs/images/My File.png"
        );
        assert_eq!(
            resolve_context(
                "../images/a.svg",
                "~/project/docs/README.md",
                Some("~/project")
            ),
            "~/project/images/a.svg"
        );
        assert_eq!(
            resolve_context("literal%20name", "docs/README.md", Some("/work/tau")),
            "/work/tau/docs/literal%20name"
        );
        assert_eq!(
            directory_label("C:\\Work\\Tau\\src", Some("c:\\work\\tau")),
            "src"
        );
        assert_eq!(
            directory_label("/work/tau-other/src", Some("/work/tau")),
            "/work/tau-other/src"
        );
        assert_eq!(directory_for_path("/README.md", Some("/work/tau")), "/");
        assert_eq!(directory_label("/", Some("/")), ".");
    }
}
