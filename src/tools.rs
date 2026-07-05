//! Tool definitions and handlers.

use crate::slim;
use crate::tokens::{estimate_tokens, saved_pct, truncate_tokens};
use regex::RegexBuilder;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const SKIP_DIRS: &[&str] = &[
    ".git", ".hg", ".svn", "node_modules", "target", "dist", "build", "out",
    ".next", ".nuxt", ".venv", "venv", "__pycache__", "vendor", ".idea",
    ".vscode", ".cache", "coverage", ".terraform", "Pods", "DerivedData",
];
const MAX_FILE_BYTES: u64 = 2_000_000;
const MAX_FILES_SCANNED: usize = 20_000;

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn s_arg(a: &Value, k: &str) -> Option<String> {
    a.get(k).and_then(|v| v.as_str()).map(|s| s.to_string())
}
fn u_arg(a: &Value, k: &str) -> Option<usize> {
    a.get(k).and_then(|v| v.as_u64()).map(|v| v as usize)
}
fn b_arg(a: &Value, k: &str) -> Option<bool> {
    a.get(k).and_then(|v| v.as_bool())
}

pub fn tool_definitions() -> Value {
    json!([
        {
            "name": "read_slim",
            "description": "Read a file with token-slimming. mode=slim (default) strips comments and collapses blank lines; mode=outline returns only function/class/heading signatures with line numbers; mode=raw returns text as-is. Output is capped at max_tokens (head+tail kept, middle snipped). Use this INSTEAD of a plain file read to save tokens.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path (absolute, or relative to server cwd)"},
                    "mode": {"type": "string", "enum": ["slim", "outline", "raw"], "description": "Default: slim"},
                    "max_tokens": {"type": "integer", "description": "Output token cap (default: env TOKEN_SLIM_MAX_TOKENS or 4000)"},
                    "offset": {"type": "integer", "description": "1-based start line"},
                    "limit": {"type": "integer", "description": "Number of lines from offset"},
                    "strip_comments": {"type": "boolean", "description": "mode=slim only. Default true"}
                },
                "required": ["path"]
            },
            "annotations": {"readOnlyHint": true, "openWorldHint": false}
        },
        {
            "name": "grep_slim",
            "description": "Regex search over a directory tree with minimal output: 'path:line:matched-line' only, capped result count, binary/vendor dirs skipped. Use this INSTEAD of a plain grep/search to save tokens.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex (Rust syntax). Set literal=true to match verbatim"},
                    "path": {"type": "string", "description": "Root dir or single file. Default: cwd"},
                    "ext": {"type": "string", "description": "Comma-separated extension filter, e.g. 'rs,toml'"},
                    "max_results": {"type": "integer", "description": "Default: env TOKEN_SLIM_GREP_MAX_RESULTS or 50"},
                    "ignore_case": {"type": "boolean", "description": "Default false"},
                    "literal": {"type": "boolean", "description": "Treat pattern as literal text. Default false"}
                },
                "required": ["pattern"]
            },
            "annotations": {"readOnlyHint": true, "openWorldHint": false}
        },
        {
            "name": "dir_map",
            "description": "Compact directory tree (one line per entry, file sizes, vendor dirs skipped, entry-capped). Use this INSTEAD of ls -R / find to save tokens.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Root dir. Default: cwd"},
                    "depth": {"type": "integer", "description": "Max depth. Default 3"},
                    "max_entries": {"type": "integer", "description": "Total entry cap. Default: env TOKEN_SLIM_DIR_MAX_ENTRIES or 300"}
                }
            },
            "annotations": {"readOnlyHint": true, "openWorldHint": false}
        },
        {
            "name": "json_slim",
            "description": "Minify and prune JSON (from text or file): depth limit, arrays sampled to first N items, long strings truncated. Ideal for large API responses / package-lock style files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "json": {"type": "string", "description": "Inline JSON text (use this OR path)"},
                    "path": {"type": "string", "description": "Path to a JSON file (use this OR json)"},
                    "max_depth": {"type": "integer", "description": "Default: env TOKEN_SLIM_JSON_MAX_DEPTH or 6"},
                    "max_array": {"type": "integer", "description": "Items kept per array. Default: env TOKEN_SLIM_JSON_MAX_ARRAY or 20"},
                    "max_string": {"type": "integer", "description": "Chars kept per string. Default: env TOKEN_SLIM_JSON_MAX_STRING or 200"}
                }
            },
            "annotations": {"readOnlyHint": true, "openWorldHint": false}
        },
        {
            "name": "text_slim",
            "description": "Compress arbitrary text: trim trailing whitespace, collapse blank lines; level=aggressive also collapses inner space runs and repeated lines. Use before quoting long logs/output back into context.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "Text to compress"},
                    "level": {"type": "string", "enum": ["normal", "aggressive"], "description": "Default: normal"},
                    "max_tokens": {"type": "integer", "description": "Output token cap (default: env TOKEN_SLIM_MAX_TOKENS or 4000)"}
                },
                "required": ["text"]
            },
            "annotations": {"readOnlyHint": true, "openWorldHint": false}
        },
        {
            "name": "token_count",
            "description": "Estimate token count of text or a file (heuristic: ascii/4 + non-ascii, ±20%, model-agnostic). Use to decide whether something is worth pasting into context.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "Inline text (use this OR path)"},
                    "path": {"type": "string", "description": "File path (use this OR text)"}
                }
            },
            "annotations": {"readOnlyHint": true, "openWorldHint": false}
        }
    ])
}

pub fn call(params: &Value) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "missing tool name".to_string()))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let handled = match name {
        "read_slim" => read_slim(&args),
        "grep_slim" => grep_slim(&args),
        "dir_map" => dir_map(&args),
        "json_slim" => json_slim(&args),
        "text_slim" => text_slim(&args),
        "token_count" => token_count(&args),
        other => return Err((-32602, format!("unknown tool: {other}"))),
    };
    Ok(match handled {
        Ok(text) => json!({"content": [{"type": "text", "text": text}]}),
        Err(e) => json!({"content": [{"type": "text", "text": format!("error: {e}")}], "isError": true}),
    })
}

fn read_text_file(path: &str) -> Result<String, String> {
    let raw = fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    if raw.len() as u64 > MAX_FILE_BYTES * 5 {
        return Err(format!("{path}: file too large ({} bytes)", raw.len()));
    }
    if raw.iter().take(4096).any(|b| *b == 0) {
        return Err(format!("{path}: binary file"));
    }
    Ok(String::from_utf8_lossy(&raw).into_owned())
}

fn ext_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

fn read_slim(a: &Value) -> Result<String, String> {
    let path = s_arg(a, "path").ok_or("path is required")?;
    let text = read_text_file(&path)?;
    let orig_tok = estimate_tokens(&text);
    let total_lines = text.lines().count();

    let mut work: String = if a.get("offset").is_some() || a.get("limit").is_some() {
        let off = u_arg(a, "offset").unwrap_or(1).max(1);
        let lim = u_arg(a, "limit").unwrap_or(usize::MAX);
        text.lines()
            .skip(off - 1)
            .take(lim)
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        text.clone()
    };

    let mode = s_arg(a, "mode").unwrap_or_else(|| "slim".into());
    let ext = ext_of(&path);
    work = match mode.as_str() {
        "outline" => slim::outline(&work, &ext),
        "raw" => work,
        _ => {
            let mut t = work;
            if b_arg(a, "strip_comments").unwrap_or(true) {
                t = slim::strip_comments(&t, &ext);
            }
            slim::collapse_blank(&t)
        }
    };

    let cap = u_arg(a, "max_tokens").unwrap_or_else(|| env_usize("TOKEN_SLIM_MAX_TOKENS", 4000));
    let (final_text, truncated) = truncate_tokens(&work, cap);
    let new_tok = estimate_tokens(&final_text);
    Ok(format!(
        "[token-slim] {path} mode={mode} lines={total_lines} ~{orig_tok}→~{new_tok} tok ({}){}\n{final_text}",
        saved_pct(orig_tok, new_tok),
        if truncated { " [capped]" } else { "" }
    ))
}

fn text_slim(a: &Value) -> Result<String, String> {
    let text = s_arg(a, "text").ok_or("text is required")?;
    let orig_tok = estimate_tokens(&text);
    let level = s_arg(a, "level").unwrap_or_else(|| "normal".into());
    let mut work = slim::collapse_blank(&text);
    if level == "aggressive" {
        work = slim::collapse_inner_spaces(&work);
        work = slim::dedupe_lines(&work);
    }
    let cap = u_arg(a, "max_tokens").unwrap_or_else(|| env_usize("TOKEN_SLIM_MAX_TOKENS", 4000));
    let (final_text, truncated) = truncate_tokens(&work, cap);
    let new_tok = estimate_tokens(&final_text);
    Ok(format!(
        "[token-slim] text level={level} ~{orig_tok}→~{new_tok} tok ({}){}\n{final_text}",
        saved_pct(orig_tok, new_tok),
        if truncated { " [capped]" } else { "" }
    ))
}

fn json_slim(a: &Value) -> Result<String, String> {
    let text = match (s_arg(a, "json"), s_arg(a, "path")) {
        (Some(j), _) => j,
        (None, Some(p)) => read_text_file(&p)?,
        (None, None) => return Err("provide either 'json' or 'path'".into()),
    };
    let orig_tok = estimate_tokens(&text);
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("invalid JSON: {e}"))?;
    let opts = slim::JsonOpts {
        max_depth: u_arg(a, "max_depth").unwrap_or_else(|| env_usize("TOKEN_SLIM_JSON_MAX_DEPTH", 6)),
        max_array: u_arg(a, "max_array").unwrap_or_else(|| env_usize("TOKEN_SLIM_JSON_MAX_ARRAY", 20)),
        max_string: u_arg(a, "max_string").unwrap_or_else(|| env_usize("TOKEN_SLIM_JSON_MAX_STRING", 200)),
    };
    let pruned = slim::prune_json(&v, opts.max_depth, &opts);
    let out = serde_json::to_string(&pruned).map_err(|e| e.to_string())?;
    let new_tok = estimate_tokens(&out);
    Ok(format!(
        "[token-slim] json depth≤{} array≤{} string≤{} ~{orig_tok}→~{new_tok} tok ({})\n{out}",
        opts.max_depth,
        opts.max_array,
        opts.max_string,
        saved_pct(orig_tok, new_tok)
    ))
}

fn token_count(a: &Value) -> Result<String, String> {
    let (label, text) = match (s_arg(a, "text"), s_arg(a, "path")) {
        (Some(t), _) => ("text".to_string(), t),
        (None, Some(p)) => (p.clone(), read_text_file(&p)?),
        (None, None) => return Err("provide either 'text' or 'path'".into()),
    };
    let chars = text.chars().count();
    let ascii = text.chars().filter(|c| c.is_ascii()).count();
    let lines = text.lines().count();
    Ok(format!(
        "[token-slim] {label}: ≈{} tokens (chars={chars}, ascii={ascii}, non-ascii={}, lines={lines}) heuristic ±20%",
        estimate_tokens(&text),
        chars - ascii
    ))
}

fn should_skip_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name) || (name.starts_with('.') && name != "." && name != "..")
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) {
    if files.len() >= MAX_FILES_SCANNED {
        return;
    }
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            if !should_skip_dir(&name) {
                dirs.push(p);
            }
        } else if ft.is_file() {
            files.push(p);
            if files.len() >= MAX_FILES_SCANNED {
                return;
            }
        }
    }
    dirs.sort();
    for d in dirs {
        collect_files(&d, files);
    }
}

fn grep_slim(a: &Value) -> Result<String, String> {
    let pattern = s_arg(a, "pattern").ok_or("pattern is required")?;
    let root = s_arg(a, "path").unwrap_or_else(|| ".".into());
    let ignore_case = b_arg(a, "ignore_case").unwrap_or(false);
    let literal = b_arg(a, "literal").unwrap_or(false);
    let max_results =
        u_arg(a, "max_results").unwrap_or_else(|| env_usize("TOKEN_SLIM_GREP_MAX_RESULTS", 50));
    let exts: Option<Vec<String>> = s_arg(a, "ext").map(|e| {
        e.split(',')
            .map(|x| x.trim().trim_start_matches('.').to_lowercase())
            .filter(|x| !x.is_empty())
            .collect()
    });

    let pat = if literal { regex::escape(&pattern) } else { pattern.clone() };
    let re = RegexBuilder::new(&pat)
        .case_insensitive(ignore_case)
        .build()
        .map_err(|e| format!("invalid regex: {e}"))?;

    let root_path = Path::new(&root);
    let mut files: Vec<PathBuf> = Vec::new();
    if root_path.is_file() {
        files.push(root_path.to_path_buf());
    } else if root_path.is_dir() {
        collect_files(root_path, &mut files);
    } else {
        return Err(format!("no such path: {root}"));
    }

    let mut results: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    let mut capped = false;
    'outer: for file in &files {
        if let Some(ref want) = exts {
            let e = file
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !want.contains(&e) {
                continue;
            }
        }
        let meta = match fs::metadata(file) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let raw = match fs::read(file) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if raw.iter().take(4096).any(|b| *b == 0) {
            continue;
        }
        scanned += 1;
        let content = String::from_utf8_lossy(&raw);
        let rel = file
            .strip_prefix(root_path)
            .unwrap_or(file)
            .to_string_lossy()
            .to_string();
        let rel = if rel.is_empty() { file.to_string_lossy().to_string() } else { rel };
        for (ln, line) in content.lines().enumerate() {
            if re.is_match(line) {
                let mut disp = line.trim().to_string();
                if disp.chars().count() > 200 {
                    disp = disp.chars().take(200).collect::<String>() + "…";
                }
                results.push(format!("{rel}:{}:{disp}", ln + 1));
                if results.len() >= max_results {
                    capped = true;
                    break 'outer;
                }
            }
        }
    }

    let header = format!(
        "[token-slim] grep /{pattern}/ in {root}: {} matches, {scanned} files scanned{}",
        results.len(),
        if capped {
            format!(" [capped at {max_results} — narrow the pattern or add ext filter]")
        } else {
            String::new()
        }
    );
    if results.is_empty() {
        Ok(header)
    } else {
        Ok(format!("{header}\n{}", results.join("\n")))
    }
}

fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn dir_map(a: &Value) -> Result<String, String> {
    let root = s_arg(a, "path").unwrap_or_else(|| ".".into());
    let depth = u_arg(a, "depth").unwrap_or(3);
    let max_entries =
        u_arg(a, "max_entries").unwrap_or_else(|| env_usize("TOKEN_SLIM_DIR_MAX_ENTRIES", 300));
    let root_path = Path::new(&root);
    if !root_path.is_dir() {
        return Err(format!("not a directory: {root}"));
    }
    let mut lines: Vec<String> = Vec::new();
    let mut count = 0usize;
    walk_map(root_path, 0, depth, max_entries, &mut lines, &mut count);
    let header = format!("[token-slim] dir_map {root} depth≤{depth} entries={count}");
    Ok(format!("{header}\n{}", lines.join("\n")))
}

fn walk_map(
    dir: &Path,
    level: usize,
    max_depth: usize,
    max_entries: usize,
    lines: &mut Vec<String>,
    count: &mut usize,
) {
    if level >= max_depth || *count >= max_entries {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut dirs: Vec<(String, PathBuf)> = Vec::new();
    let mut files: Vec<(String, u64)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            if !should_skip_dir(&name) {
                dirs.push((name, entry.path()));
            }
        } else if ft.is_file() {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            files.push((name, size));
        }
    }
    dirs.sort();
    files.sort();
    let indent = "  ".repeat(level);
    for (name, path) in &dirs {
        if *count >= max_entries {
            lines.push(format!("{indent}… [entry cap reached]"));
            return;
        }
        lines.push(format!("{indent}{name}/"));
        *count += 1;
        walk_map(path, level + 1, max_depth, max_entries, lines, count);
    }
    for (name, size) in &files {
        if *count >= max_entries {
            lines.push(format!("{indent}… [entry cap reached]"));
            return;
        }
        lines.push(format!("{indent}{name} {}", human_size(*size)));
        *count += 1;
    }
}
