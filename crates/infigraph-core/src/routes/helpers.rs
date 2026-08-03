use super::Route;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Lang {
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    Rust,
    Ruby,
    Php,
    CSharp,
    Elixir,
    Other,
}

pub(crate) fn language_from_file(file: &str) -> Lang {
    if file.ends_with(".py") {
        Lang::Python
    } else if file.ends_with(".js") || file.ends_with(".jsx") || file.ends_with(".mjs") {
        Lang::JavaScript
    } else if file.ends_with(".ts") || file.ends_with(".tsx") {
        Lang::TypeScript
    } else if file.ends_with(".go") {
        Lang::Go
    } else if file.ends_with(".java") || file.ends_with(".kt") || file.ends_with(".scala") {
        Lang::Java
    } else if file.ends_with(".rs") {
        Lang::Rust
    } else if file.ends_with(".rb") {
        Lang::Ruby
    } else if file.ends_with(".php") {
        Lang::Php
    } else if file.ends_with(".cs") {
        Lang::CSharp
    } else if file.ends_with(".ex") || file.ends_with(".exs") {
        Lang::Elixir
    } else {
        Lang::Other
    }
}

/// True if `kw` occurs in `text` bounded by non-alphanumeric chars (or
/// start/end of string) on both sides, so e.g. "get" matches "@router.get("
/// and "GET /x" but not the "get" inside "/widgets" or "budget()".
fn contains_word(text: &str, kw: &str) -> bool {
    let is_boundary = |c: Option<char>| !matches!(c, Some(c) if c.is_alphanumeric());
    text.match_indices(kw).any(|(start, _)| {
        let end = start + kw.len();
        is_boundary(text[..start].chars().next_back()) && is_boundary(text[end..].chars().next())
    })
}

pub(crate) fn detect_from_docstring(
    id: &str,
    name: &str,
    file: &str,
    doc_lower: &str,
) -> Option<Route> {
    // Look for explicit HTTP method keywords in docstrings
    let http_methods = [
        ("get", "GET"),
        ("post", "POST"),
        ("put", "PUT"),
        ("delete", "DELETE"),
        ("patch", "PATCH"),
    ];

    // Pattern: docstring mentions route/endpoint/api along with an HTTP method
    let has_route_context = doc_lower.contains("route")
        || doc_lower.contains("endpoint")
        || doc_lower.contains("api")
        || doc_lower.contains("handler")
        || doc_lower.contains("@app.")
        || doc_lower.contains("@router.")
        || doc_lower.contains("handlefunc")
        || doc_lower.contains("mapping");

    if !has_route_context {
        return None;
    }

    // Try to extract method from docstring. Match as a whole word (not a
    // substring of a path segment like "widgets" or "deleted") by requiring
    // a non-alphanumeric char (or start-of-string) on both sides.
    let method = http_methods
        .iter()
        .find(|(kw, _)| contains_word(doc_lower, kw))
        .map(|(_, m)| m.to_string())
        .unwrap_or_else(|| "GET".to_string());

    // Try to extract a path from the docstring (look for /something patterns)
    let path =
        extract_path_from_text(doc_lower).unwrap_or_else(|| format!("/{}", name.to_lowercase()));

    Some(Route {
        method,
        path,
        handler_id: id.to_string(),
        file: file.to_string(),
        framework: detect_framework_from_docstring(doc_lower),
    })
}

pub(crate) fn detect_go_framework(doc_lower: &str) -> String {
    if doc_lower.contains("gin.") || doc_lower.contains("gin ") {
        "gin".to_string()
    } else if doc_lower.contains("echo.") {
        "echo".to_string()
    } else if doc_lower.contains("chi.") {
        "chi".to_string()
    } else if doc_lower.contains("fiber") {
        "fiber".to_string()
    } else if doc_lower.contains("mux") || doc_lower.contains("gorilla") {
        "gorilla/mux".to_string()
    } else {
        "net/http".to_string()
    }
}

/// Best-effort framework label for a `Route`-kind (registration) symbol, keyed
/// off the source file extension since the registration site has no decorator
/// docstring to inspect.
pub(crate) fn detect_framework_from_file(file: &str) -> String {
    match language_from_file(file) {
        Lang::Python => "python".to_string(),
        Lang::JavaScript | Lang::TypeScript => "express".to_string(),
        Lang::Go => "net/http".to_string(),
        Lang::Java => "spring".to_string(),
        Lang::Rust => "rust".to_string(),
        Lang::Ruby => "rails".to_string(),
        Lang::Php => "laravel".to_string(),
        Lang::CSharp => "aspnet".to_string(),
        Lang::Elixir => "phoenix".to_string(),
        Lang::Other => "unknown".to_string(),
    }
}

pub(crate) fn detect_framework_from_docstring(doc_lower: &str) -> String {
    if doc_lower.contains("flask") || doc_lower.contains("@app.") {
        "flask".to_string()
    } else if doc_lower.contains("fastapi") {
        "fastapi".to_string()
    } else if doc_lower.contains("django") {
        "django".to_string()
    } else if doc_lower.contains("express") {
        "express".to_string()
    } else if doc_lower.contains("nestjs") {
        "nestjs".to_string()
    } else if doc_lower.contains("spring") || doc_lower.contains("mapping") {
        "spring".to_string()
    } else if doc_lower.contains("actix") {
        "actix".to_string()
    } else if doc_lower.contains("axum") {
        "axum".to_string()
    } else if doc_lower.contains("rocket") {
        "rocket".to_string()
    } else if doc_lower.contains("gin.") {
        "gin".to_string()
    } else if doc_lower.contains("rails") {
        "rails".to_string()
    } else if doc_lower.contains("laravel") {
        "laravel".to_string()
    } else if doc_lower.contains("phoenix") {
        "phoenix".to_string()
    } else if doc_lower.contains("handlefunc") || doc_lower.contains("http.handle") {
        "net/http".to_string()
    } else {
        "generic".to_string()
    }
}

/// Try to extract a URL path (e.g., /users/{id}) from text.
pub(crate) fn extract_path_from_text(text: &str) -> Option<String> {
    // Look for patterns like "/something" or '/something'
    for delim in ['"', '\''] {
        if let Some(start) = text.find(&format!("{}/", delim)) {
            let path_start = start + 1; // skip the delimiter
            if let Some(end) = text[path_start..].find(delim) {
                let path = &text[path_start..path_start + end];
                if path.starts_with('/') && path.len() > 1 {
                    return Some(path.to_string());
                }
            }
        }
    }

    // Look for unquoted /path patterns (e.g., in docstrings: "GET /users")
    for word in text.split_whitespace() {
        if word.starts_with('/') && word.len() > 1 && !word.starts_with("//") {
            return Some(word.to_string());
        }
    }

    None
}

/// Infer HTTP method from a name (e.g., "create_user" -> "POST").
pub(crate) fn infer_method_from_name(name: &str) -> String {
    if name.starts_with("get")
        || name.starts_with("list")
        || name.starts_with("find")
        || name.starts_with("fetch")
        || name.starts_with("read")
        || name.starts_with("show")
        || name.starts_with("index")
    {
        "GET".to_string()
    } else if name.starts_with("create")
        || name.starts_with("add")
        || name.starts_with("post")
        || name.starts_with("save")
        || name.starts_with("new")
    {
        "POST".to_string()
    } else if name.starts_with("update")
        || name.starts_with("put")
        || name.starts_with("edit")
        || name.starts_with("modify")
    {
        "PUT".to_string()
    } else if name.starts_with("delete")
        || name.starts_with("remove")
        || name.starts_with("destroy")
    {
        "DELETE".to_string()
    } else if name.starts_with("patch") {
        "PATCH".to_string()
    } else {
        "UNKNOWN".to_string()
    }
}

/// Convert camelCase to a URL path segment: "userProfile" -> "user/profile".
pub(crate) fn camel_to_path(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.char_indices() {
        if c.is_uppercase() && i > 0 {
            result.push('/');
            result.push(c.to_lowercase().next().unwrap_or(c));
        } else {
            result.push(c);
        }
    }
    // Also convert underscores to slashes
    result.replace('_', "/")
}
