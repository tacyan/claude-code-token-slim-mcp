//! Tool definitions and handlers.

use crate::glob::any_match;
use crate::refs::{self, RefKind, SymbolPatterns};
use crate::slim;
use crate::tokens::{estimate_tokens, saved_pct, truncate_tokens};
use regex::RegexBuilder;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".venv",
    "venv",
    "__pycache__",
    "vendor",
    ".idea",
    ".vscode",
    ".cache",
    "coverage",
    ".terraform",
    "Pods",
    "DerivedData",
];
const MAX_FILE_BYTES: u64 = 2_000_000;
const MAX_FILES_SCANNED: usize = 20_000;

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_str(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
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
/// Accept either ["a","b"] or "a,b" for list-valued arguments.
fn list_arg(a: &Value, k: &str) -> Vec<String> {
    match a.get(k) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) => s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Common test/spec/fixture layouts, expanded by `exclude_tests: true`.
const TEST_GLOBS: &[&str] = &[
    "**/test/**",
    "**/tests/**",
    "**/__tests__/**",
    "**/spec/**",
    "**/specs/**",
    "**/testdata/**",
    "**/fixtures/**",
    "**/__mocks__/**",
    "**/e2e/**",
    "*_test.*",
    "*_tests.*",
    "test_*.*",
    "*.test.*",
    "*.spec.*",
    "*Test.java",
    "*Tests.cs",
    "conftest.py",
];

pub fn tool_definitions() -> Value {
    json!([
        {
            "name": "read_slim",
            "description": "Read a file with token-slimming. mode=auto (DEFAULT) returns a structure outline (function/class/heading signatures + line numbers, ~-95%) for large code files and falls back to slim for small or unstructured files — read the outline first, then fetch the parts you need with offset/limit. mode=slim strips comments and collapses blank lines; mode=outline forces the outline; mode=raw returns text as-is. Passing offset/limit always reads that range (never outlined). Output is capped at max_tokens (head+tail kept, middle snipped). Use this INSTEAD of a plain file read to save tokens.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path (absolute, or relative to server cwd)"},
                    "mode": {"type": "string", "enum": ["auto", "slim", "outline", "raw"], "description": "Default: auto (outline for large code files, slim otherwise; env TOKEN_SLIM_DEFAULT_MODE overrides)"},
                    "max_tokens": {"type": "integer", "description": "Output token cap (default: env TOKEN_SLIM_MAX_TOKENS or 4000)"},
                    "offset": {"type": "integer", "description": "1-based start line"},
                    "limit": {"type": "integer", "description": "Number of lines from offset"},
                    "strip_comments": {"type": "boolean", "description": "slim/auto only. Default true"}
                },
                "required": ["path"]
            },
            "annotations": {"readOnlyHint": true, "openWorldHint": false}
        },
        {
            "name": "grep_slim",
            "description": "Regex search over a directory tree with minimal output: 'path:line:matched-line' only, capped result count, binary/vendor dirs skipped. Use exclude globs (or exclude_tests) to drop test/fixture noise — usually the single biggest saving on a structural query. Use this INSTEAD of a plain grep/search to save tokens.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex (Rust syntax). Set literal=true to match verbatim"},
                    "path": {"type": "string", "description": "Root dir or single file. Default: cwd"},
                    "ext": {"type": "string", "description": "Comma-separated extension filter, e.g. 'rs,toml'"},
                    "max_results": {"type": "integer", "description": "Default: env TOKEN_SLIM_GREP_MAX_RESULTS or 50"},
                    "ignore_case": {"type": "boolean", "description": "Default false"},
                    "literal": {"type": "boolean", "description": "Treat pattern as literal text. Default false"},
                    "exclude": {"type": "array", "items": {"type": "string"}, "description": "Glob patterns to skip, e.g. [\"**/test/**\", \"*.spec.ts\"]. Supports * ? and **; a slash-free pattern matches any path segment, and a multi-segment pattern may also match deeper in the tree (tests/** also excludes crates/x/tests/). A comma-separated string is accepted too"},
                    "exclude_tests": {"type": "boolean", "description": "Shorthand for the usual test/spec/fixture globs (test/, tests/, __tests__/, spec/, testdata/, fixtures/, *_test.*, *.spec.*, ...). Default false"},
                    "include": {"type": "array", "items": {"type": "string"}, "description": "If set, only paths matching one of these globs are searched (applied before exclude)"}
                },
                "required": ["pattern"]
            },
            "annotations": {"readOnlyHint": true, "openWorldHint": false}
        },
        {
            "name": "refs_slim",
            "description": "Find every reference to a symbol and CLASSIFY it: definition, call, test-call, import, comment, or bare mention. Answers 'who calls this?' — which a plain grep cannot, because it cannot tell a call from the declaration, an import, or a test. Reports counts for every class and lists only the definition and the real call sites, each attributed to the function that contains it. depth=2 also reports the callers of those functions (the blast radius of a change). No index, so it never goes stale.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "symbol": {"type": "string", "description": "Identifier to trace, e.g. 'computeFrameComp'"},
                    "path": {"type": "string", "description": "Root dir or single file. Default: cwd"},
                    "ext": {"type": "string", "description": "Comma-separated extension filter, e.g. 'ts,tsx'"},
                    "depth": {"type": "integer", "description": "1 = direct callers (default). 2 = also the callers of those, for a change's blast radius"},
                    "max_results": {"type": "integer", "description": "Cap on listed call sites. Default: env TOKEN_SLIM_GREP_MAX_RESULTS or 50"},
                    "include_tests": {"type": "boolean", "description": "List test call sites too instead of only counting them. Default false"},
                    "exclude": {"type": "array", "items": {"type": "string"}, "description": "Glob patterns to skip (same syntax as grep_slim)"},
                    "include": {"type": "array", "items": {"type": "string"}, "description": "If set, only paths matching one of these globs are searched"}
                },
                "required": ["symbol"]
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
            "description": "Minify and prune JSON/JSONC (from text or file): depth limit, arrays sampled to first N items, long strings truncated. JSONC input (// and /* */ comments, trailing commas — bun.lock, tsconfig.json, .vscode/*.json) is normalized automatically. Ideal for large API responses / lockfile-style files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "json": {"type": "string", "description": "Inline JSON/JSONC text (use this OR path)"},
                    "path": {"type": "string", "description": "Path to a JSON/JSONC file (use this OR json)"},
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
        "refs_slim" => refs_slim(&args),
        "dir_map" => dir_map(&args),
        "json_slim" => json_slim(&args),
        "text_slim" => text_slim(&args),
        "token_count" => token_count(&args),
        other => return Err((-32602, format!("unknown tool: {other}"))),
    };
    Ok(match handled {
        Ok(text) => json!({"content": [{"type": "text", "text": text}]}),
        Err(e) => {
            json!({"content": [{"type": "text", "text": format!("error: {e}")}], "isError": true})
        }
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
    let ranged = a.get("offset").is_some() || a.get("limit").is_some();

    let base: String = if ranged {
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

    let ext = ext_of(&path);
    let strip = b_arg(a, "strip_comments").unwrap_or(true);
    let slimmed = || {
        let t = if strip {
            slim::strip_comments(&base, &ext)
        } else {
            base.clone()
        };
        slim::collapse_blank(&t)
    };

    let cap = u_arg(a, "max_tokens").unwrap_or_else(|| env_usize("TOKEN_SLIM_MAX_TOKENS", 4000));
    let requested = s_arg(a, "mode")
        .unwrap_or_else(|| env_str("TOKEN_SLIM_DEFAULT_MODE", "auto"))
        .to_lowercase();
    let (mode_label, work) = match requested.as_str() {
        "raw" => ("raw".to_string(), base.clone()),
        "outline" => ("outline".to_string(), slim::outline(&base, &ext)),
        "slim" => ("slim".to_string(), slimmed()),
        _ => resolve_auto(&base, &ext, ranged, slimmed(), cap),
    };

    let (final_text, truncated) = truncate_tokens(&work, cap);
    let new_tok = estimate_tokens(&final_text);
    let hint = if mode_label.starts_with("outline") {
        " — structure only, no bodies: fetch a function with offset/limit, or the whole file with mode=slim"
    } else {
        ""
    };
    Ok(format!(
        "[token-slim] {path} mode={mode_label} lines={total_lines} ~{orig_tok}→~{new_tok} tok ({}){}{hint}\n{final_text}",
        saved_pct(orig_tok, new_tok),
        if truncated { " [capped]" } else { "" }
    ))
}

/// mode=auto: prefer the outline for large, structured files; otherwise
/// return the slimmed body. A requested line range is never outlined —
/// that call is already the drill-down step.
///
/// The outline wins when it is smaller than the body AND either the body
/// would be capped anyway (a snipped middle loses more than an outline
/// does) or the file is big enough that the outline at least halves it.
fn resolve_auto(
    base: &str,
    ext: &str,
    ranged: bool,
    slim_text: String,
    cap: usize,
) -> (String, String) {
    if ranged {
        return ("slim(auto)".to_string(), slim_text);
    }
    let slim_tok = estimate_tokens(&slim_text);
    let min_tok = env_usize("TOKEN_SLIM_AUTO_OUTLINE_MIN_TOKENS", 400);
    if let Some(o) = slim::outline_opt(base, ext) {
        let o_tok = estimate_tokens(&o);
        if o_tok < slim_tok && (slim_tok > cap || (slim_tok > min_tok && o_tok * 2 <= slim_tok)) {
            return ("outline(auto)".to_string(), o);
        }
    }
    ("slim(auto)".to_string(), slim_text)
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
    // Strict JSON first; on failure retry as JSONC (comments + trailing
    // commas), which covers bun.lock / tsconfig.json / .vscode/*.json.
    let (v, jsonc): (Value, bool) = match serde_json::from_str(&text) {
        Ok(v) => (v, false),
        Err(strict_err) => {
            let relaxed = slim::strip_jsonc(&text);
            match serde_json::from_str(&relaxed) {
                Ok(v) => (v, true),
                Err(jsonc_err) => {
                    let se = strict_err.to_string();
                    let je = jsonc_err.to_string();
                    return Err(if se == je {
                        format!("invalid JSON: {se}")
                    } else {
                        format!("invalid JSON: {se} (also unparsable as JSONC: {je})")
                    });
                }
            }
        }
    };
    let opts = slim::JsonOpts {
        max_depth: u_arg(a, "max_depth")
            .unwrap_or_else(|| env_usize("TOKEN_SLIM_JSON_MAX_DEPTH", 6)),
        max_array: u_arg(a, "max_array")
            .unwrap_or_else(|| env_usize("TOKEN_SLIM_JSON_MAX_ARRAY", 20)),
        max_string: u_arg(a, "max_string")
            .unwrap_or_else(|| env_usize("TOKEN_SLIM_JSON_MAX_STRING", 200)),
    };
    let pruned = slim::prune_json(&v, opts.max_depth, &opts);
    let out = serde_json::to_string(&pruned).map_err(|e| e.to_string())?;
    let new_tok = estimate_tokens(&out);
    Ok(format!(
        "[token-slim] json{} depth≤{} array≤{} string≤{} ~{orig_tok}→~{new_tok} tok ({})\n{out}",
        if jsonc {
            "c (comments/trailing commas stripped)"
        } else {
            ""
        },
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

/// Path of `p` relative to `root`, `/`-separated, for glob matching.
fn rel_path(root: &Path, p: &Path) -> String {
    let rel = p
        .strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .to_string();
    if rel.is_empty() {
        p.to_string_lossy().to_string()
    } else {
        rel
    }
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<PathBuf>, exclude: &[String]) {
    if files.len() >= MAX_FILES_SCANNED {
        return;
    }
    let entries = match fs::read_dir(dir) {
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
            // Prune excluded directories instead of walking them.
            if !should_skip_dir(&name) && !any_match(exclude, &rel_path(root, &p)) {
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
        collect_files(root, &d, files, exclude);
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
    let include = list_arg(a, "include");
    let mut exclude = list_arg(a, "exclude");
    if b_arg(a, "exclude_tests").unwrap_or(false) {
        exclude.extend(TEST_GLOBS.iter().map(|g| g.to_string()));
    }

    let pat = if literal {
        regex::escape(&pattern)
    } else {
        pattern.clone()
    };
    let re = RegexBuilder::new(&pat)
        .case_insensitive(ignore_case)
        .build()
        .map_err(|e| format!("invalid regex: {e}"))?;

    let root_path = Path::new(&root);
    let mut files: Vec<PathBuf> = Vec::new();
    if root_path.is_file() {
        files.push(root_path.to_path_buf());
    } else if root_path.is_dir() {
        collect_files(root_path, root_path, &mut files, &exclude);
    } else {
        return Err(format!("no such path: {root}"));
    }

    let mut results: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    let mut skipped = 0usize;
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
        let rel = rel_path(root_path, file);
        if !include.is_empty() && !any_match(&include, &rel) {
            skipped += 1;
            continue;
        }
        if any_match(&exclude, &rel) {
            skipped += 1;
            continue;
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

    // Excluded directories are pruned during the walk, so `skipped` counts
    // only file-level filtering — name the active filters as well, so an
    // empty result is never mistaken for "nothing matches anywhere".
    let mut filters: Vec<String> = Vec::new();
    if !include.is_empty() {
        filters.push(format!("include={}", include.join(",")));
    }
    if !exclude.is_empty() {
        let shown: Vec<&str> = exclude.iter().take(3).map(|s| s.as_str()).collect();
        let more = exclude.len().saturating_sub(shown.len());
        filters.push(format!(
            "exclude={}{}",
            shown.join(","),
            if more > 0 {
                format!(",+{more}")
            } else {
                String::new()
            }
        ));
    }
    if skipped > 0 {
        filters.push(format!("{skipped} files skipped"));
    }
    let filtered = if filters.is_empty() {
        String::new()
    } else {
        format!(", {}", filters.join(", "))
    };
    let header = format!(
        "[token-slim] grep /{pattern}/ in {root}: {} matches, {scanned} files scanned{filtered}{}",
        results.len(),
        if capped {
            format!(
                " [capped at {max_results} — narrow the pattern, add ext filter or exclude globs]"
            )
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

/// One file's text, read once and reused across every scan pass so `depth=2`
/// costs a second classification, not a second walk.
struct LoadedFile {
    rel: String,
    content: String,
    is_test: bool,
}

/// A classified reference to the symbol being traced.
struct Hit {
    rel: String,
    line: usize,
    text: String,
    kind: RefKind,
    /// The function/class whose body contains this line, when one does.
    enclosing: Option<String>,
}

/// Read every candidate file once. Binary, oversized and filtered files drop
/// out here, so the scan passes see only text they can classify.
fn load_files(a: &Value, root_path: &Path) -> Result<Vec<LoadedFile>, String> {
    let include = list_arg(a, "include");
    let exclude = list_arg(a, "exclude");
    let exts: Option<Vec<String>> = s_arg(a, "ext").map(|e| {
        e.split(',')
            .map(|x| x.trim().trim_start_matches('.').to_lowercase())
            .filter(|x| !x.is_empty())
            .collect()
    });

    let mut paths: Vec<PathBuf> = Vec::new();
    if root_path.is_file() {
        paths.push(root_path.to_path_buf());
    } else if root_path.is_dir() {
        collect_files(root_path, root_path, &mut paths, &exclude);
    } else {
        return Err(format!("no such path: {}", root_path.display()));
    }

    let mut out = Vec::new();
    for file in paths {
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
        let rel = rel_path(root_path, &file);
        if !include.is_empty() && !any_match(&include, &rel) {
            continue;
        }
        if any_match(&exclude, &rel) {
            continue;
        }
        match fs::metadata(&file) {
            Ok(m) if m.len() <= MAX_FILE_BYTES => {}
            _ => continue,
        }
        let raw = match fs::read(&file) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if raw.iter().take(4096).any(|b| *b == 0) {
            continue;
        }
        let is_test = any_match(
            &TEST_GLOBS.iter().map(|g| g.to_string()).collect::<Vec<_>>(),
            &rel,
        );
        out.push(LoadedFile {
            rel,
            content: String::from_utf8_lossy(&raw).into_owned(),
            is_test,
        });
    }
    Ok(out)
}

/// Classify every mention of `sym` across the loaded files. `resolve_enclosing`
/// is the expensive half (it re-walks a file to track brace depth), so it runs
/// only for the hits that will actually be reported or followed.
fn scan_symbol(
    files: &[LoadedFile],
    sym: &str,
    resolve_enclosing: bool,
) -> Result<Vec<Hit>, String> {
    let pats = SymbolPatterns::new(sym)?;
    let word = regex::Regex::new(&format!(r"\b{}\b", regex::escape(sym)))
        .map_err(|e| format!("bad symbol {sym:?}: {e}"))?;
    let mut hits = Vec::new();
    for f in files {
        if !word.is_match(&f.content) {
            continue; // whole-file reject: most files never mention the symbol
        }
        for (i, line) in f.content.lines().enumerate() {
            if !word.is_match(line) {
                continue;
            }
            let kind = pats.classify(line, f.is_test);
            let enclosing =
                if resolve_enclosing && matches!(kind, RefKind::Call | RefKind::TestCall) {
                    refs::enclosing_symbol(&f.content, i + 1)
                } else {
                    None
                };
            let mut text = line.trim().to_string();
            if text.chars().count() > 160 {
                text = text.chars().take(160).collect::<String>() + "…";
            }
            hits.push(Hit {
                rel: f.rel.clone(),
                line: i + 1,
                text,
                kind,
                enclosing,
            });
        }
    }
    Ok(hits)
}

fn refs_slim(a: &Value) -> Result<String, String> {
    let symbol = s_arg(a, "symbol").ok_or("symbol is required")?;
    let root = s_arg(a, "path").unwrap_or_else(|| ".".into());
    let depth = u_arg(a, "depth").unwrap_or(1).clamp(1, 2);
    let include_tests = b_arg(a, "include_tests").unwrap_or(false);
    let max_results =
        u_arg(a, "max_results").unwrap_or_else(|| env_usize("TOKEN_SLIM_GREP_MAX_RESULTS", 50));

    let root_path = Path::new(&root);
    let files = load_files(a, root_path)?;
    let hits = scan_symbol(&files, &symbol, true)?;

    let mut counts: Vec<(RefKind, usize)> = Vec::new();
    for k in RefKind::ORDER {
        let n = hits.iter().filter(|h| h.kind == k).count();
        if n > 0 {
            counts.push((k, n));
        }
    }
    let summary = if counts.is_empty() {
        "no references".to_string()
    } else {
        counts
            .iter()
            .map(|(k, n)| format!("{} {n}", k.label()))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let mut body: Vec<String> = Vec::new();
    let mut listed = 0usize;
    let mut capped = false;
    for k in [RefKind::Definition, RefKind::Call, RefKind::TestCall] {
        if k == RefKind::TestCall && !include_tests {
            continue;
        }
        for h in hits.iter().filter(|h| h.kind == k) {
            if listed >= max_results {
                capped = true;
                break;
            }
            let tag = match k {
                RefKind::Definition => "def ",
                RefKind::Call => "call",
                _ => "test",
            };
            let scope = match &h.enclosing {
                Some(e) if Some(e.as_str()) != Some(symbol.as_str()) => format!("  in {e}"),
                _ => String::new(),
            };
            body.push(format!("{tag} {}:{}{scope}: {}", h.rel, h.line, h.text));
            listed += 1;
        }
    }

    // depth=2: the callers of the functions that call `symbol` — the set a
    // change to `symbol` can reach. Names only; the hop-1 lines above already
    // carry the exact locations.
    if depth >= 2 {
        let mut seen: Vec<String> = vec![symbol.clone()];
        let mut frontier: Vec<String> = hits
            .iter()
            .filter(|h| h.kind == RefKind::Call)
            .filter_map(|h| h.enclosing.clone())
            .collect();
        frontier.sort();
        frontier.dedup();
        frontier.retain(|f| !seen.contains(f));
        seen.extend(frontier.iter().cloned());

        let mut hop2: Vec<String> = Vec::new();
        for caller in &frontier {
            let inner = scan_symbol(&files, caller, true)?;
            for h in inner.iter().filter(|h| h.kind == RefKind::Call) {
                let via = h.enclosing.clone().unwrap_or_else(|| "(top level)".into());
                if via == *caller {
                    continue; // recursion, not a new caller
                }
                let entry = format!("hop2 {via} -> {caller}  ({}:{})", h.rel, h.line);
                if !hop2.contains(&entry) {
                    hop2.push(entry);
                }
            }
        }
        hop2.sort();
        if hop2.is_empty() {
            body.push("hop2 (no further callers)".to_string());
        } else {
            body.extend(hop2);
        }
    }

    let depth_note = if depth >= 2 { " depth=2" } else { "" };
    let header = format!(
        "[token-slim] refs {symbol} in {root}{depth_note}: {summary}; {} files scanned{}",
        files.len(),
        if capped {
            format!(" [listing capped at {max_results}]")
        } else {
            String::new()
        }
    );
    if body.is_empty() {
        Ok(header)
    } else {
        Ok(format!("{header}\n{}", body.join("\n")))
    }
}
