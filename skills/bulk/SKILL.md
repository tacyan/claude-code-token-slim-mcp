---
name: bulk
description: Bulk mode — maximum development speed by spending tokens aggressively on massive parallel sub-agents and multi-agent workflows, while the orchestrator context stays lean (pairs with /slim). Invoking /bulk is the user's explicit opt-in to Workflow/ultracode-style orchestration for this session. Use when the user types /bulk, asks for "爆速", "並列で大量に", "ultracode", "多エージェント", or hands over a big task where wall-clock speed matters more than token cost.
---

# /bulk — Bulk Mode (session-wide, pairs with /slim)

Trade-off: the OPPOSITE of /slim on spend, the SAME on context hygiene.
Burn tokens in parallel, disposable agent contexts to compress wall-clock
time; keep the main (orchestrator) context slim so the session never drowns.
The task description (if any) was passed as the skill arguments — treat it
as the mission and start immediately.

**Opt-in**: the user invoking /bulk IS the explicit opt-in for multi-agent
orchestration (the Workflow tool) for this session. Don't re-ask permission
to fan out; do respect normal gates (commit/push/publish/destructive ops).

## Step 1 — Engage

1. **Pair with /slim** (mandatory): if slim mode is not already active this
   session, activate it now — read `~/.claude/skills/slim/SKILL.md` if it
   exists, else apply its core rule directly: the orchestrator context takes
   only conclusions, never raw file dumps. Bulk without slim floods the
   conductor and kills the speed you bought.
2. **Capability check** (adapt, never fail): Agent tool → always fan out with
   it. Workflow tool → use for deterministic loops/pipelines. Neither
   restriction applies to folder structure: discover any repo via
   `dir_map` / an Explore agent — no hardcoded paths, works in anyone's tree.
3. Confirm to the user in ONE line (their language): bulk mode ON,
   slim pairing status, and the fan-out plan for the mission (N agents / phases).

## Step 2 — Orchestrator rules (MANDATORY until session end)

1. **You are the conductor, not a worker.** Inline work is limited to:
   scoping the work-list (`dir_map`, `grep_slim`, quick scout), integrating
   results, and tiny mechanical edits. Everything else — exploration,
   implementation, tests, review — goes to agents.
2. **Parallel-first.** Independent work NEVER runs sequentially: batch all
   independent Agent calls in ONE message. Default 3–8 concurrent agents;
   scale up for audits/migrations. Honor "+500k"-style budget directives via
   `budget` in Workflow scripts.
3. **Pick the right vehicle:**
   - 2–8 independent units, model-driven → parallel `Agent` calls
     (Explore for read-only sweeps, general-purpose/specialist for builds,
     `fork` when the worker needs this conversation's context).
   - Deterministic fan-out, loops, verify-gates, >8 units → `Workflow`
     (`pipeline()` by default; barriers only for cross-item dedup/merge;
     `isolation: 'worktree'` whenever agents mutate files in parallel —
     git repos only: in a plain non-git folder, partition the files
     disjointly between agents or serialize the mutating stage instead).
4. **Slim footer** — append to EVERY sub-agent prompt:

   > Context discipline: prefer token-slim tools (ToolSearch
   > "select:mcp__token-slim__read_slim,mcp__token-slim__grep_slim,mcp__token-slim__dir_map",
   > then read_slim/grep_slim/dir_map instead of Read/Grep/ls; outline mode
   > first on big files). If unavailable, cap Bash output with `| head -50`.
   > Final message = conclusions only (findings, diffs applied, file:line
   > refs, test evidence), ≤30 lines, NO file dumps.

5. **Speed discipline:** while agents run, prepare the next phase (prompts,
   work-lists) instead of idling; don't re-run what an agent already did;
   relay agent conclusions to the user — their output is invisible otherwise.
6. **Evidence gate (speed never skips it):** every implementation agent must
   build/test its own unit and report the output; findings from review/audit
   fan-outs get adversarial verification (independent skeptic agents, majority
   rules) before being reported as real; one final verify pass exercises the
   integrated result end-to-end.

## Step 3 — Playbooks (compose freely)

- **EXPLORE** (map an unknown repo in one round): `dir_map` for the skeleton,
  then N parallel Explore agents, one per subsystem → merge into a map.
- **BUILD** (feature at max speed): scout the seams inline → split into
  independent units → parallel implementation agents in worktrees (or on
  disjoint file sets when the folder is not a git repo), each
  writing+running its own tests → integrate → one verify agent end-to-end.
- **AUDIT/REVIEW** (be exhaustive): Workflow — finder agents per dimension
  (bugs/security/perf/tests) → dedup → 3-skeptic adversarial verify per
  finding → loop until 2 consecutive dry rounds.
- **MIGRATE** (N call-sites): inline `grep_slim` discovers the site list →
  Workflow `pipeline(sites, transform, verify)` with worktree isolation
  (non-git folder: partition sites by file, or serialize the transform stage).
- **DESIGN** (wide solution space): 3 parallel agents propose from different
  angles (MVP-first / risk-first / user-first) → judge panel scores →
  synthesize the winner, graft the best runner-up ideas.

## Reporting

Outcome first: what shipped/was found, verified how, and the aggregate
(N agents, phases, wall-clock). Then only the details that change what the
reader does next. These rules survive context compaction — re-read this
skill if you lose them.
