//! Lossy text/code slimming: comment stripping, blank collapsing,
//! outline extraction, JSON pruning. Output is for LLM context, not for
//! compilation, so edge cases (raw strings, docstrings) may be imperfect.

use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

pub struct CommentStyle {
    pub line: &'static [&'static str],
    pub block: &'static [(&'static str, &'static str)],
}

pub fn comment_style(ext: &str) -> CommentStyle {
    match ext {
        "rs" | "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "java" | "c" | "h" | "cpp"
        | "cc" | "hpp" | "go" | "swift" | "kt" | "kts" | "scala" | "cs" | "dart" | "php"
        | "css" | "scss" | "less" | "proto" | "zig" | "m" | "mm" => CommentStyle {
            line: &["//"],
            block: &[("/*", "*/")],
        },
        "py" | "rb" | "sh" | "bash" | "zsh" | "fish" | "yaml" | "yml" | "toml" | "pl"
        | "r" | "jl" | "ex" | "exs" | "tf" | "nix" | "mk" | "cmake" | "dockerfile"
        | "gitignore" | "env" => CommentStyle {
            line: &["#"],
            block: &[],
        },
        "sql" => CommentStyle {
            line: &["--"],
            block: &[("/*", "*/")],
        },
        "lua" => CommentStyle {
            line: &["--"],
            block: &[("--[[", "]]")],
        },
        "html" | "htm" | "xml" | "vue" | "svelte" | "svg" => CommentStyle {
            line: &[],
            block: &[("<!--", "-->")],
        },
        _ => CommentStyle {
            line: &[],
            block: &[],
        },
    }
}

fn starts_with_at(chars: &[char], i: usize, pat: &str) -> bool {
    let mut j = i;
    for pc in pat.chars() {
        if j >= chars.len() || chars[j] != pc {
            return false;
        }
        j += 1;
    }
    true
}

/// Strip comments with rough string-literal awareness.
pub fn strip_comments(src: &str, ext: &str) -> String {
    let style = comment_style(ext);
    if style.line.is_empty() && style.block.is_empty() {
        return src.to_string();
    }
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let mut in_str: Option<char> = None;

    while i < n {
        let c = chars[i];
        if let Some(q) = in_str {
            out.push(c);
            if c == '\\' && i + 1 < n {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        if let Some((open, close)) = style
            .block
            .iter()
            .find(|(o, _)| starts_with_at(&chars, i, o))
        {
            i += open.chars().count();
            while i < n && !starts_with_at(&chars, i, close) {
                if chars[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            if i < n {
                i += close.chars().count();
            }
            continue;
        }
        if let Some(_lc) = style.line.iter().find(|p| starts_with_at(&chars, i, p)) {
            // keep shebang line
            if i == 0 && starts_with_at(&chars, i, "#!") {
                while i < n && chars[i] != '\n' {
                    out.push(chars[i]);
                    i += 1;
                }
                continue;
            }
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '"' || c == '\'' || c == '`' {
            in_str = Some(c);
            out.push(c);
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Trim trailing whitespace and collapse runs of blank lines to one.
pub fn collapse_blank(src: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut blank_run = 0usize;
    for line in src.lines() {
        let t = line.trim_end();
        if t.is_empty() {
            blank_run += 1;
            if blank_run <= 1 && !out.is_empty() {
                out.push("");
            }
        } else {
            blank_run = 0;
            out.push(t);
        }
    }
    while out.last() == Some(&"") {
        out.pop();
    }
    out.join("\n")
}

/// Collapse runs of >= 3 identical lines into two + a repeat marker.
pub fn dedupe_lines(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let mut j = i + 1;
        while j < lines.len() && lines[j] == lines[i] {
            j += 1;
        }
        let run = j - i;
        if run >= 3 && !lines[i].trim().is_empty() {
            out.push(lines[i].to_string());
            out.push(format!("… (same line ×{run})"));
        } else {
            for k in i..j {
                out.push(lines[k].to_string());
            }
        }
        i = j;
    }
    out.join("\n")
}

/// Collapse internal whitespace runs (keeps leading indentation).
pub fn collapse_inner_spaces(src: &str) -> String {
    src.lines()
        .map(|line| {
            let indent_len = line.len() - line.trim_start().len();
            let (indent, rest) = line.split_at(indent_len);
            let mut collapsed = String::with_capacity(rest.len());
            let mut prev_space = false;
            for c in rest.chars() {
                if c == ' ' || c == '\t' {
                    if !prev_space {
                        collapsed.push(' ');
                    }
                    prev_space = true;
                } else {
                    prev_space = false;
                    collapsed.push(c);
                }
            }
            format!("{indent}{}", collapsed.trim_end())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

static SIG_RE: OnceLock<Regex> = OnceLock::new();
static ARROW_RE: OnceLock<Regex> = OnceLock::new();
static MD_RE: OnceLock<Regex> = OnceLock::new();

fn sig_re() -> &'static Regex {
    SIG_RE.get_or_init(|| {
        Regex::new(
            r#"^\s*(?:(?:pub(?:\([^)]*\))?|export|default|public|private|protected|internal|package|static|async|abstract|final|sealed|override|open|extern(?:\s+"[^"]*")?|unsafe|inline|virtual|declare|partial|data|suspend)\s+)*(?:fn|func|function|def|class|interface|struct|enum|trait|impl|type|protocol|extension|module|namespace|object|record|macro_rules!)\b"#,
        )
        .unwrap()
    })
}

fn arrow_re() -> &'static Regex {
    ARROW_RE.get_or_init(|| {
        Regex::new(
            r"^\s*(?:export\s+)?(?:const|let|var)\s+[A-Za-z_$][\w$]*\s*(?::[^=]+)?=\s*(?:async\s*)?(?:\([^)]*\)|[A-Za-z_$][\w$]*)\s*=>",
        )
        .unwrap()
    })
}

fn md_re() -> &'static Regex {
    MD_RE.get_or_init(|| Regex::new(r"^#{1,6}\s").unwrap())
}

/// Extract signature/heading lines with line numbers.
pub fn outline(src: &str, ext: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let is_md = matches!(ext, "md" | "mdx" | "markdown");
    for (i, line) in src.lines().enumerate() {
        let hit = if is_md {
            md_re().is_match(line)
        } else {
            sig_re().is_match(line) || arrow_re().is_match(line)
        };
        if hit {
            let mut disp = line.trim_end().to_string();
            if disp.chars().count() > 160 {
                disp = disp.chars().take(160).collect::<String>() + "…";
            }
            out.push(format!("L{}: {}", i + 1, disp));
        }
    }
    if out.is_empty() {
        return "(no signatures/headings found — try mode=slim)".to_string();
    }
    out.join("\n")
}

pub struct JsonOpts {
    pub max_depth: usize,
    pub max_array: usize,
    pub max_string: usize,
}

/// Prune a JSON value: depth limit, array sampling, string truncation.
pub fn prune_json(v: &Value, depth: usize, o: &JsonOpts) -> Value {
    match v {
        Value::String(s) => {
            let len = s.chars().count();
            if len > o.max_string {
                Value::String(format!(
                    "{}…[+{} chars]",
                    s.chars().take(o.max_string).collect::<String>(),
                    len - o.max_string
                ))
            } else {
                v.clone()
            }
        }
        Value::Array(arr) => {
            if depth == 0 {
                return Value::String(format!("[…{} items]", arr.len()));
            }
            let mut out: Vec<Value> = arr
                .iter()
                .take(o.max_array)
                .map(|x| prune_json(x, depth - 1, o))
                .collect();
            if arr.len() > o.max_array {
                out.push(Value::String(format!(
                    "…+{} more items",
                    arr.len() - o.max_array
                )));
            }
            Value::Array(out)
        }
        Value::Object(m) => {
            if depth == 0 {
                return Value::String(format!("{{…{} keys}}", m.len()));
            }
            let mut out = serde_json::Map::new();
            for (k, val) in m {
                out.insert(k.clone(), prune_json(val, depth - 1, o));
            }
            Value::Object(out)
        }
        _ => v.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strips_rust_comments() {
        let src = "// header\nfn main() { /* block */ let s = \"// not a comment\"; }\n";
        let out = strip_comments(src, "rs");
        assert!(!out.contains("header"));
        assert!(!out.contains("block"));
        assert!(out.contains("// not a comment"));
        assert!(out.contains("fn main()"));
    }

    #[test]
    fn strips_python_comments_keeps_shebang() {
        let src = "#!/usr/bin/env python\n# comment\nx = 1  # trailing\ns = \"# not comment\"\n";
        let out = strip_comments(src, "py");
        assert!(out.contains("#!/usr/bin/env python"));
        assert!(!out.contains("# comment"));
        assert!(!out.contains("# trailing"));
        assert!(out.contains("\"# not comment\""));
    }

    #[test]
    fn collapses_blanks() {
        let out = collapse_blank("a\n\n\n\nb   \n\n");
        assert_eq!(out, "a\n\nb");
    }

    #[test]
    fn dedupes_repeats() {
        let out = dedupe_lines("x\nx\nx\nx\ny");
        assert!(out.contains("… (same line ×4)"));
        assert!(out.contains('y'));
    }

    #[test]
    fn outline_finds_signatures() {
        let src = "use std;\n\npub fn hello(a: u32) -> u32 {\n  a\n}\nstruct Foo;\nconst X: u8 = 1;\n";
        let out = outline(src, "rs");
        assert!(out.contains("L3: pub fn hello"));
        assert!(out.contains("L6: struct Foo;"));
        assert!(!out.contains("use std"));
    }

    #[test]
    fn outline_markdown_headings() {
        let out = outline("# Title\ntext\n## Sub\n", "md");
        assert!(out.contains("L1: # Title"));
        assert!(out.contains("L3: ## Sub"));
    }

    #[test]
    fn prunes_json() {
        let v = json!({
            "big": (0..100).collect::<Vec<i32>>(),
            "long": "x".repeat(500),
            "deep": {"a": {"b": {"c": {"d": 1}}}}
        });
        let o = JsonOpts { max_depth: 3, max_array: 5, max_string: 10 };
        let p = prune_json(&v, o.max_depth, &o);
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains("…+95 more items"));
        assert!(s.contains("…[+490 chars]"));
        assert!(s.contains("…1 keys") || s.contains("{…1 keys}"));
    }
}
