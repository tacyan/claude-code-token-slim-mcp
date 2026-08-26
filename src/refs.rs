//! Symbol reference classification for `refs_slim`.
//!
//! A plain grep for a symbol answers "where does this string appear", which is
//! rarely the question. Asked who calls `computeFrameComp`, grep returns 36
//! lines across a real repository: 27 in tests, 3 imports, 1 definition — and
//! 2 actual call sites buried in the middle. Classifying each hit by its shape
//! turns that list back into the question that was asked, and costs one pass
//! over the same bytes grep already read.
//!
//! Everything here is pure: no filesystem, no MCP types. The walk lives in
//! `tools.rs`, so the classification and the scope resolver stay unit-testable.

use crate::slim;
use regex::Regex;

/// What a line does with the symbol it mentions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// `function foo(`, `class foo`, `const foo =` — where it is declared.
    Definition,
    /// `foo(...)` in production code.
    Call,
    /// `foo(...)` inside a test/spec/fixture file.
    TestCall,
    /// `import { foo }`, `export { foo }`, `require('…')`.
    Import,
    /// The mention sits in a comment.
    Comment,
    /// The name appears without being called — a type position, a string, a
    /// property key.
    Mention,
}

impl RefKind {
    /// Stable short label used in the tool's output header.
    pub fn label(self) -> &'static str {
        match self {
            RefKind::Definition => "definition",
            RefKind::Call => "call",
            RefKind::TestCall => "test-call",
            RefKind::Import => "import",
            RefKind::Comment => "comment",
            RefKind::Mention => "mention",
        }
    }

    /// Report order: the answer first, the noise last.
    pub const ORDER: [RefKind; 6] = [
        RefKind::Definition,
        RefKind::Call,
        RefKind::TestCall,
        RefKind::Import,
        RefKind::Comment,
        RefKind::Mention,
    ];
}

/// Per-symbol patterns, compiled once per search rather than per line.
pub struct SymbolPatterns {
    symbol: String,
    call: Regex,
    def: Regex,
    import: Regex,
}

impl SymbolPatterns {
    /// Build the matchers for `sym`. Fails only if the symbol contains
    /// characters that break the escape, which `regex::escape` prevents.
    pub fn new(sym: &str) -> Result<Self, String> {
        let e = regex::escape(sym);
        let call = Regex::new(&format!(r"\b{e}\s*(?:<[^>()]*>)?\s*\("))
            .map_err(|err| format!("bad symbol {sym:?}: {err}"))?;
        // Keyword declarations only. A keyword-less method (`  frame(): void {`)
        // is recognised through `slim::is_signature` instead, so `refs_slim`
        // and the outline can never disagree about what a declaration is — and
        // a bare top-level call like `widen(1)` in a test file is not mistaken
        // for one, which a "bare identifier + (" pattern does.
        let def = Regex::new(&format!(
            r"\b(?:fn|func|function|def|class|interface|struct|enum|trait|type|const|let|var|static)\s+{e}\b"
        ))
        .map_err(|err| format!("bad symbol {sym:?}: {err}"))?;
        let import = Regex::new(r"^\s*(?:import\b|export\b.*\bfrom\b|(?:const|let|var)\s.*\brequire\s*\()|^\s*(?:from\s+\S+\s+import\b)|^\s*use\s")
            .map_err(|err| format!("import pattern: {err}"))?;
        Ok(Self {
            symbol: sym.to_string(),
            call,
            def,
            import,
        })
    }

    /// Classify the reference on line `i`. The surrounding lines are needed
    /// because a declaration whose argument list wraps (`private constructor(`)
    /// is the same shape as a call whose arguments wrap (`registerHandler(`);
    /// only the line that closes the list tells them apart. `in_test_file`
    /// comes from the path, not the content, so a helper named `test` in
    /// production code is not miscounted.
    pub fn classify(&self, lines: &[&str], i: usize, in_test_file: bool) -> RefKind {
        let line = lines[i];
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') || t.starts_with("#") {
            return RefKind::Comment;
        }
        if self.import.is_match(line) {
            return RefKind::Import;
        }
        // A declaration also has the shape of a call (`  frame(dt: number) {`),
        // so it has to win the tie — otherwise every method would be reported
        // as calling itself.
        if self.def.is_match(line) {
            return RefKind::Definition;
        }
        if slim::is_signature_at(lines, i, false)
            && signature_name(line).as_deref() == Some(self.symbol.as_str())
        {
            return RefKind::Definition;
        }
        if self.call.is_match(line) {
            return if in_test_file {
                RefKind::TestCall
            } else {
                RefKind::Call
            };
        }
        RefKind::Mention
    }
}

/// Net `{` minus `}` on a line, ignoring braces inside line comments, string
/// literals and character literals. Crude next to a parser, but it only has to
/// keep the scope stack aligned, and it never reads a brace that a compiler
/// would not.
fn net_delims(line: &str) -> (i32, i32) {
    let bytes: Vec<char> = line.chars().collect();
    let mut braces = 0i32;
    let mut parens = 0i32;
    let mut i = 0usize;
    let mut quote: Option<char> = None;
    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            Some(q) => {
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == '/' {
                    break; // line comment: nothing after it counts
                }
                match c {
                    '"' | '\'' | '`' => quote = Some(c),
                    '{' => braces += 1,
                    '}' => braces -= 1,
                    '(' => parens += 1,
                    ')' => parens -= 1,
                    _ => {}
                }
            }
        }
        i += 1;
    }
    (braces, parens)
}

/// The identifier a signature line declares, or `None` when the line has no
/// usable name (an anonymous default export, a bare `impl` block).
pub fn signature_name(line: &str) -> Option<String> {
    let kw = Regex::new(
        r"\b(?:fn|func|function|def|class|interface|struct|enum|trait|type|impl|namespace|module|object|record)\s+([A-Za-z_$][\w$]*)",
    )
    .ok()?;
    if let Some(c) = kw.captures(line) {
        return Some(c[1].to_string());
    }
    let binding = Regex::new(r"\b(?:const|let|var|static)\s+([A-Za-z_$][\w$]*)").ok()?;
    if let Some(c) = binding.captures(line) {
        return Some(c[1].to_string());
    }
    // Keyword-less method: modifiers, then the name, then an argument list.
    let method = Regex::new(
        r"^[ \t]*(?:(?:pub|public|private|protected|internal|static|async|abstract|override|final|readonly|get|set|open|suspend)\s+)*(?:\*\s*)?([A-Za-z_$][\w$]*)\s*(?:<[^>()]*>)?\s*\(",
    )
    .ok()?;
    method.captures(line).map(|c| c[1].to_string())
}

/// Name of the function, method or class whose body contains `line_no`
/// (1-based), tracking brace depth so an inner arrow binding that has already
/// closed cannot claim a later line.
///
/// This is what makes `depth > 1` worth having: resolving a call site back to
/// its enclosing symbol is the step that turns "line 697 of renderer.ts" into
/// "`frame`", and a naive "nearest signature above" scan gets it wrong exactly
/// where it matters — inside classes, which is where methods live.
pub fn enclosing_symbol(src: &str, line_no: usize) -> Option<String> {
    let mut stack: Vec<(String, i32)> = Vec::new();
    let mut depth = 0i32;
    // A signature whose argument list wraps (`private constructor(` … `) {`)
    // opens its body several lines later, so the name has to be held until the
    // brace that belongs to it arrives — otherwise its calls get attributed to
    // the enclosing class. The hold is gated on the argument list still being
    // open: without that, a complete one-line binding such as
    // `const su = (n: string) => this.u(n)` would stay pending and claim the
    // next unrelated `{` in the file.
    let mut pending: Option<String> = None;
    let mut open_parens = 0i32;
    let lines: Vec<&str> = src.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if i + 1 > line_no {
            break;
        }
        let name = if slim::is_signature_at(&lines, i, false) {
            signature_name(line)
        } else {
            None
        };
        let before = depth;
        let (d_brace, d_paren) = net_delims(line);
        depth += d_brace;
        open_parens = (open_parens + d_paren).max(0);
        if let Some(nm) = name {
            // Only a signature that opens a body owns a scope; a bodyless
            // interface member declares nothing that can contain a call.
            if depth > before {
                stack.push((nm, before));
                pending = None;
            } else if open_parens > 0 {
                pending = Some(nm);
            } else {
                pending = None;
            }
        } else if depth > before && open_parens == 0 {
            // The held name only owns this brace if the brace arrives on the
            // line that closes its argument list (`) {`). Anything opened
            // while the list is still open belongs to something inside it —
            // a callback body, not the declaration.
            if let Some(nm) = pending.take() {
                stack.push((nm, before));
            }
        } else if open_parens == 0 {
            pending = None;
        }
        while let Some((_, opened_at)) = stack.last() {
            if depth <= *opened_at {
                stack.pop();
            } else {
                break;
            }
        }
    }
    stack.last().map(|(n, _)| n.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pats() -> SymbolPatterns {
        SymbolPatterns::new("computeFrameComp").unwrap()
    }

    #[test]
    fn classifies_the_shapes_grep_cannot_tell_apart() {
        let p = pats();
        assert_eq!(
            p.classify(
                &["export function computeFrameComp(input: I): O {"],
                0,
                false
            ),
            RefKind::Definition
        );
        assert_eq!(
            p.classify(&["  const { a } = computeFrameComp({ dt })"], 0, false),
            RefKind::Call
        );
        assert_eq!(
            p.classify(&["  expect(computeFrameComp(x)).toBe(1)"], 0, true),
            RefKind::TestCall
        );
        assert_eq!(
            p.classify(&["import { computeFrameComp } from './m'"], 0, false),
            RefKind::Import
        );
        assert_eq!(
            p.classify(&["  // computeFrameComp is shared"], 0, false),
            RefKind::Comment
        );
        assert_eq!(
            p.classify(&["  type T = typeof computeFrameComp"], 0, false),
            RefKind::Mention
        );
    }

    #[test]
    fn a_method_declaration_is_not_a_call_to_itself() {
        let p = SymbolPatterns::new("frame").unwrap();
        assert_eq!(
            p.classify(&["  frame(dt: number, time: number): void {"], 0, false),
            RefKind::Definition
        );
        assert_eq!(
            p.classify(&["    this.renderer.frame(dt, time)"], 0, false),
            RefKind::Call
        );
    }

    #[test]
    fn net_delims_ignores_strings_and_comments() {
        assert_eq!(net_delims("class A {").0, 1);
        assert_eq!(net_delims("}").0, -1);
        assert_eq!(net_delims(r#"const s = "{{{" // }}}"#).0, 0);
        assert_eq!(net_delims("const t = `${x}`").0, 0);
        assert_eq!(net_delims("if (a) { b() } else { c() }").0, 0);
    }

    #[test]
    fn signature_name_reads_every_declaration_shape() {
        assert_eq!(
            signature_name("export function foo(a: T) {").as_deref(),
            Some("foo")
        );
        assert_eq!(signature_name("export class Bar {").as_deref(), Some("Bar"));
        assert_eq!(
            signature_name("  private syncLookModes(): void {").as_deref(),
            Some("syncLookModes")
        );
        assert_eq!(
            signature_name("  frame(dt: number): void {").as_deref(),
            Some("frame")
        );
        assert_eq!(
            signature_name("const blurBG = (v: V) =>").as_deref(),
            Some("blurBG")
        );
    }

    #[test]
    fn enclosing_symbol_survives_a_closed_inner_binding() {
        // The bug this replaces: a "nearest signature above" scan answers
        // `blurBG` for line 9, because it never notices that blurBG's body
        // closed on line 6.
        let src = "\
export class R {
  private setup(): void {
    const blurBG = (v: V) => {
      use(v)
    }
    blurBG(x)
  }
  frame(dt: number): void {
    compute({ dt })
  }
}
";
        assert_eq!(enclosing_symbol(src, 4).as_deref(), Some("blurBG"));
        assert_eq!(enclosing_symbol(src, 6).as_deref(), Some("setup"));
        assert_eq!(enclosing_symbol(src, 9).as_deref(), Some("frame"));
        // A line sitting directly in a class body — a field initializer, or
        // the declaration itself — resolves to the class.
        assert_eq!(enclosing_symbol(src, 1).as_deref(), Some("R"));
    }

    #[test]
    fn enclosing_symbol_follows_a_wrapped_argument_list() {
        // `private constructor(` opens its body two lines later. Losing the
        // pending name attributes the call to the class, which is the wrong
        // answer at exactly the granularity refs_slim exists to provide.
        let src = "\
export class R {
  private constructor(
    private readonly device: GPUDevice,
  ) {
    this.blend = resolveBlendMode(opts.blendMode)
  }
}
";
        assert_eq!(enclosing_symbol(src, 5).as_deref(), Some("constructor"));
        // Line 6 closes the constructor, so the class owns it again.
        assert_eq!(enclosing_symbol(src, 6).as_deref(), Some("R"));
    }

    #[test]
    fn a_closed_one_line_binding_never_claims_a_later_brace() {
        // `const su = (n) => this.u(n)` is complete on its own line. Holding it
        // as a pending signature made it swallow the next unrelated `{` and
        // report `su` as the caller of everything below it.
        let src = "\
export class R {
  frame(dt: number): void {
    const su = (name: string) => this.u('sim', name)
    su('uDt')
    const { a } = computeFrameComp({
      trail: this.look.trail,
    })
    use(a)
  }
}
";
        assert_eq!(enclosing_symbol(src, 4).as_deref(), Some("frame"));
        assert_eq!(enclosing_symbol(src, 6).as_deref(), Some("frame"));
        assert_eq!(enclosing_symbol(src, 8).as_deref(), Some("frame"));
    }

    #[test]
    fn a_wrapped_call_does_not_own_the_brace_its_callback_opens() {
        // `registerHandler(` is the same shape as `static async create(`, so
        // holding it pending made it claim the brace its own callback opened
        // and report `registerHandler` as the caller of everything inside.
        let src = "\
export class C {
  run(): void {
    registerHandler(
      'name',
      () => {
        target(1)
      },
    )
    target(2)
  }
}
";
        assert_eq!(enclosing_symbol(src, 6).as_deref(), Some("run"));
        assert_eq!(enclosing_symbol(src, 9).as_deref(), Some("run"));
    }

    #[test]
    fn enclosing_symbol_is_none_above_every_scope() {
        let src = "import { a } from './a'\n\nexport function f(): void {\n  a()\n}\n";
        assert_eq!(enclosing_symbol(src, 1), None);
        assert_eq!(enclosing_symbol(src, 4).as_deref(), Some("f"));
    }

    #[test]
    fn enclosing_symbol_handles_top_level_functions() {
        let src = "\
export function alpha(): void {
  work()
}

export function beta(): void {
  work()
}
";
        assert_eq!(enclosing_symbol(src, 2).as_deref(), Some("alpha"));
        assert_eq!(enclosing_symbol(src, 6).as_deref(), Some("beta"));
    }
}
