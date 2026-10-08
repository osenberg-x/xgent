# Outcome

Produce a prioritized gap report for the XGent agent implementation: a single ranked list of what the agent still lacks, backed by code evidence (file:line), covering both design-level (planned but unbuilt) and implementation-level (built but defective/unwired) gaps.

# Scope

## Coverage boundary

Whole-agent audit, treated as three evidence sources:

| Source item and location | Read status | Content to preserve | Spec location | Acceptance ID | Coverage status | Reason or replacement |
| --- | --- | --- | --- | --- | --- | --- |
| S1: User request "梳理这个 agent 实现还缺少哪些功能，按照优先级列出" | complete | Ranked missing-capability list for the agent | `specs/agent-gap-report/spec.md` | A1, A2, A3 | covered | Primary requirement |
| S2: `doc/design/requirements.md` (F-xx / NF-xx / P1 / P2 roadmap) | complete | Planned-but-unimplemented scope and phase labels | `specs/agent-gap-report/spec.md` | A1, A4 | covered | Design requirement source |
| S3: `doc/design/architecture.md` (trait contracts, daemon uplift, D-xx) | complete | Architecture promises with no code implementation | `specs/agent-gap-report/spec.md` | A1, A4 | covered | Design requirement source |
| S4: `doc/design/plugin-system-design.md` (WIT extension points, D-P xx) | complete | Plugin extension points declared but unimplemented | `specs/agent-gap-report/spec.md` | A1, A4 | covered | Design requirement source |
| S5: `doc/plans/*` (step1~step12, O1~O10, ui-*-plans) | complete | Remaining unchecked plan tasks | `specs/agent-gap-report/spec.md` | A1, A4 | covered | Design requirement source |
| S6: `doc/dev-tutorial.md` (implemented-feature overview, ADR landing points) | complete | Baseline of what is actually done | `specs/agent-gap-report/spec.md` | A1, A4 | covered | Design requirement source |
| S7: `doc/notes/*` (research reports, pending decisions) | complete | D-xx / OQ-xx blockers | `specs/agent-gap-report/spec.md` | A4 | covered | Design requirement source |
| S8: `crates/**` Rust sources (19 crates) | complete | Code evidence for implementation-level defects | `specs/agent-gap-report/spec.md` | A1, A2, A5 | covered | Implementation source, audited via subagents |
| S9: `doc/README.md` and other plan docs with stale status | complete | Document-vs-code inconsistencies (ADR list only to 0010, crate count, UI v7 marked pending) | `specs/agent-gap-report/spec.md` | A4 | background | Recorded as a finding class, not an implementation requirement for this change |
| S10: Out-of-scope: 3D / TUI / Web / pet product implementations | n/a | n/a | n/a | n/a | non-goal | P2 roadmap, excluded from audit depth |

## Deliverable

One report document under `doc/` (language: Chinese, per AGENTS.md §2), containing the ranked gap list with evidence and rationale.

# Non-goals

- No implementation of any missing capability in this change. Report only.
- No 3D / TUI / Web / pet implementation work.
- No change to Cargo manifests, dependencies, or runtime behavior.
- No rewrite of `requirements.md` / `architecture.md`; doc inconsistencies are reported as findings, not fixed here.

# Constraints and invariants

- Findings must cite `file:line` evidence from the current tree, or a named document section for design-level gaps.
- Ranked list must be non-duplicative: one gap appears once, with its cross-references merged.
- Distinguish "planned but unbuilt" from "built but defective or unwired"; a gap that fits both is labeled by its actual code state.

# Decisions

- D-R1: Report-only change. The user's request asks for a prioritized list, not implementation.
- D-R2: Audit depth covers all 19 crates, not just MVP scope, because "还缺少哪些功能" spans the whole agent.
- D-R3: Report language Chinese; Comet formal artifacts stay `en` per `.comet/config.yaml`.
- D-R4: Deliverable is a single document at `doc/notes/agent-gap-review.md`. No follow-up implementation plans in this change.
- D-R5: Ranking axis is risk-and-blocking: safety / runaway-cost / unbounded-loop first, then MVP-scope acceptance blockers, then experience improvements, then P1/P2 roadmap.
- D-R6: All four gap classes are covered: planned-but-unbuilt, built-but-defective, unwired-design, doc-inconsistency.

# Acceptance examples

- A1: `doc/notes/agent-gap-review.md` exists and contains one non-duplicative ranked gap list. Each entry has: rank, gap title, gap class label (planned-not-built / built-but-defective / unwired-design / doc-inconsistency), one-line description, and evidence — `file:line` for implementation gaps, or a named document section for design-level gaps.
- A2: The ranking is ordered by risk-and-blocking first: safety, runaway-cost, and unbounded-loop defects occupy the top ranks; MVP-scope acceptance blockers follow; experience improvements and P1/P2 roadmap items follow last. The document states this ordering rule explicitly.
- A3: Every implementation gap asserted in the document cites a `file:line` that exists in the current tree, and no gap is listed twice — cross-cutting defects appear as a single entry that names all affected call sites.
- A4: All four gap classes are present: planned-but-unbuilt features (from requirements.md / plugin-system-design.md), built-but-defective implementations, architecture designs declared but unwired in code, and documentation-vs-code inconsistencies. Each class is visually distinguishable in the document.
- A5: Every P0/MVP requirement item from `doc/design/requirements.md` that is claimed implemented is either absent from the report or accompanied by contradicting code evidence, so the report does not silently inherit the existing documentation claims. The document does not modify any existing document under `doc/`.