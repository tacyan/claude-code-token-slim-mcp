//! Token estimation heuristic (no external tokenizer, model-agnostic).
//! ASCII text averages ~4 chars/token across GPT/Claude/Gemini tokenizers;
//! CJK and other non-ASCII chars average ~1 token/char. Accuracy ±20%.

pub fn estimate_tokens(s: &str) -> usize {
    let mut ascii = 0usize;
    let mut other = 0usize;
    for c in s.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
    }
    let est = ascii as f64 / 4.0 + other as f64;
    est.ceil() as usize
}

pub fn saved_pct(orig: usize, new: usize) -> String {
    if orig == 0 {
        return "±0%".into();
    }
    if new >= orig {
        return "±0%".into();
    }
    format!("-{}%", (orig - new) * 100 / orig)
}

/// Cap `text` to roughly `max` tokens, keeping head (~70%) and tail (~20%)
/// with a snip marker in between. Returns (text, was_truncated).
pub fn truncate_tokens(text: &str, max: usize) -> (String, bool) {
    if estimate_tokens(text) <= max {
        return (text.to_string(), false);
    }
    let lines: Vec<&str> = text.lines().collect();
    let head_budget = max * 7 / 10;
    let tail_budget = max * 2 / 10;

    let mut head_end = 0usize;
    let mut used = 0usize;
    for (i, l) in lines.iter().enumerate() {
        let t = estimate_tokens(l) + 1;
        if used + t > head_budget {
            break;
        }
        used += t;
        head_end = i + 1;
    }

    let mut tail_start = lines.len();
    used = 0;
    for (i, l) in lines.iter().enumerate().rev() {
        if i < head_end {
            break;
        }
        let t = estimate_tokens(l) + 1;
        if used + t > tail_budget {
            break;
        }
        used += t;
        tail_start = i;
    }

    let snipped = tail_start.saturating_sub(head_end);
    if snipped == 0 {
        return (text.to_string(), false);
    }
    let snipped_tok: usize = lines[head_end..tail_start]
        .iter()
        .map(|l| estimate_tokens(l) + 1)
        .sum();
    let mut out = String::new();
    out.push_str(&lines[..head_end].join("\n"));
    out.push_str(&format!(
        "\n… [token-slim: {snipped} lines / ~{snipped_tok} tok snipped — use offset/limit or grep_slim for the rest] …\n"
    ));
    out.push_str(&lines[tail_start..].join("\n"));
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_estimate() {
        // 40 ASCII chars -> ~10 tokens
        let s = "a".repeat(40);
        assert_eq!(estimate_tokens(&s), 10);
    }

    #[test]
    fn cjk_estimate() {
        // 10 CJK chars -> ~10 tokens
        let s = "日本語のテキストです。".chars().take(10).collect::<String>();
        assert_eq!(estimate_tokens(&s), 10);
    }

    #[test]
    fn truncate_keeps_head_and_tail() {
        let text = (0..500)
            .map(|i| format!("line number {i} with some padding text"))
            .collect::<Vec<_>>()
            .join("\n");
        let (out, truncated) = truncate_tokens(&text, 300);
        assert!(truncated);
        assert!(out.contains("line number 0 "));
        assert!(out.contains("line number 499"));
        assert!(out.contains("token-slim:"));
        assert!(estimate_tokens(&out) < estimate_tokens(&text));
    }

    #[test]
    fn no_truncate_when_small() {
        let (out, truncated) = truncate_tokens("hello\nworld", 100);
        assert!(!truncated);
        assert_eq!(out, "hello\nworld");
    }
}
