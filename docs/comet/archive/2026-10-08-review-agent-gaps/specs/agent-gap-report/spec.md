# Agent Gap Review Report

## Purpose

A single reviewable document that states, in ranked priority order, what the XGent agent implementation still lacks. The report exists so that implementation effort can be sequenced by risk and blocking impact rather than by the phase labels used during planning.

## Deliverable location and language

- The report lives at `doc/notes/agent-gap-review.md`.
- The report is written in Chinese, following AGENTS.md §2 (all design and note documents are written in Chinese).
- The report is a new file. No existing file under `doc/` is modified by this capability.

## Gap classes

Every ranked entry carries exactly one class label from this list:

- `planned-not-built`: a capability named in `doc/design/requirements.md`, `doc/design/architecture.md`, or `doc/design/plugin-system-design.md` that has no code implementation.
- `built-but-defective`: a capability that exists in code but is incomplete, unbounded, silently ignored, or contradicts its own stated contract.
- `unwired-design`: a trait contract, extension point, or daemon-uplift duty declared in the architecture or plugin-system design that no call site actually exercises.
- `doc-inconsistency`: a claim in an existing document under `doc/` that contradicts the current code, so readers get a wrong picture of what exists.

A defect that spans both `built-but-defective` and `unwired-design` is labeled by its actual code state: if the code path exists but its declared contract is never exercised, it is `unwired-design`.

## Evidence rule

- Implementation gaps cite `file:line` in the current tree. The cited location must exist.
- Design-level gaps cite a named document and section, for example `requirements.md F-12`.
- A document inconsistency cites both the document location and the contradicting `file:line`.
- Every cited `file:line` must be verifiable in the tree at report-writing time.

## Ranking rule

Ranks are assigned strictly in this order, and the report states this rule in its own text:

1. Safety, runaway-cost, and unbounded-execution defects. Examples: an agent loop with no iteration ceiling, an abort that does not cancel the upstream request, a tool that can overflow the context window in one call.
2. Defects that block acceptance of an already-confirmed MVP requirement. Examples: an MVP-listed provider adapter that is an empty shell.
3. Functional gaps that degrade everyday use but do not block MVP acceptance. Examples: retrieval that fails for the primary language.
4. P1 and P2 roadmap capability, still listed but not yet built.

Entries within one rank band are ordered by blast radius, not by implementation cost.

## De-duplication rule

A defect reachable from several call sites appears as one ranked entry. The entry names every affected call site in its evidence rather than repeating the defect per call site.

## Coverage rule

- Every `F-xx` and `NF-xx` item in `doc/design/requirements.md` that is described as implemented is re-checked against code. If the claim is contradicted, the report records the contradiction instead of accepting the document's claim.
- Every pending decision (`D-xx`, `OQ-xx`, `D-P xx`) is recorded with the capability it blocks, so a blocked feature is not read as merely unstarted.
- P2 items that are explicitly deferred and carry no implementation trigger are listed compactly, without the evidence depth of P0/P1 items.

## Non-modification rule

The report does not edit any existing document under `doc/`. Document inconsistencies become findings in the report; fixing them is separate work.

## Scenarios

### Ranked list present with class labels and evidence

Scenario: a reader opens the report and finds one ranked list
验收：A1

Given the report at `doc/notes/agent-gap-review.md`
When the reader scans the ranked list
Then every entry shows a rank, a title, a class label, a one-line description, and an evidence reference of `file:line` or a named document section

### Ordering follows risk before roadmap

Scenario: the top of the list is the most dangerous item
验收：A2

Given the report states its ranking rule
When the reader reads ranks from the top
Then unbounded-execution and cost-runaway defects precede MVP acceptance blockers, which precede everyday-use gaps, which precede P1/P2 roadmap items

### No duplicate entries and no dangling evidence

Scenario: every evidence pointer resolves
验收：A3

Given each implementation entry cites `file:line`
When the cited locations are read in the current tree
Then every cited location exists and no defect appears as more than one entry

### All four classes are distinguishable

Scenario: the reader can filter by gap class
验收：A4

Given the report contains planned-not-built, built-but-defective, unwired-design, and doc-inconsistency entries
When the reader looks for class labels
Then all four classes appear and each is visually distinguishable from the others

### Existing documents are not modified

Scenario: the audit does not alter prior documentation
验收：A5

Given the report was written
When `doc/` is inspected for changes other than the new report file
Then no pre-existing document under `doc/` was modified, and any MVP requirement claimed implemented is either absent from the report or backed by contradicting code evidence