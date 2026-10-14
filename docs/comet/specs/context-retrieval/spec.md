# Context Retrieval

## Purpose

This capability is the retrieval tier that feeds the agent's system message: it turns a user's question, the currently open file, and editor hints into a bounded set of repository excerpts. It must work for Chinese questions as well as English ones, must not consume a tokio worker thread doing filesystem IO, and must never return more context than the caller's token budget.

The behavior lives in `crates/xgent_context` (`on_demand.rs` for the built-in provider, `hub.rs` for aggregation) and is consumed by `xgent_agent` through the `ContextProvider` trait.

## Interface

The capability exposes and consumes these existing types unchanged:

- `ContextQuery { user_message, current_file, hints, max_tokens }`
- `ContextChunk { path, content, relevance, token_estimate }`
- `ContextResult { chunks, tree_summary, total_tokens }`
- `ContextProvider::retrieve(&ContextQuery) -> ContextResult`, and `ContextHub` implementing the same trait so the agent cannot tell built-in from plugin providers.

No new dependency is added for text segmentation.

## Keyword extraction

Keywords are the only bridge between a user's sentence and a repository search, so extraction must produce terms a search can actually hit.

- Text is split into runs of CJK characters and runs of ASCII word characters; separators are whitespace and punctuation of either script.
- An ASCII token is kept when it has at least three **characters**. Length is not measured in bytes.
- A CJK run of two or more characters becomes the set of overlapping two-character slices. A run of exactly one character contributes nothing.
- Listed Chinese function characters/words and common English function words are dropped from both paths.
- Results are de-duplicated, keep question order, and are capped at 16 keywords.
- Explicit `hints` still take priority over extracted keywords, and hints of the form `@file:<path>` are routed to file resolution instead of keyword search.

Acceptance: A2

Scenario: whole Chinese sentence no longer becomes one unmatched keyword
验收：A2

Given a user message of Chinese characters with no spaces
When keywords are extracted
Then each keyword is a two-character slice of a CJK run, none of them is the whole sentence, and a single-character run contributes no keyword

Scenario: length is counted in characters, not bytes
验收：A2

Given a two-character ASCII token such as `fn` alongside a three-character token such as `abc`
When keywords are extracted
Then `fn` is dropped and `abc` is kept, regardless of how many bytes either occupies

Scenario: function words are dropped
验收：A2

Given a message mixing English function words with Chinese function characters
When keywords are extracted
Then neither group appears as a keyword

Scenario: the keyword set is bounded
验收：A2

Given a question long enough to produce more than sixteen keywords
When keywords are extracted
Then at most sixteen keywords are returned, in question order, without duplicates

## Locating a referenced file

An editor `@file:` reference names a file, so it should contribute that file's content rather than a search term.

- A hint matching `@file:<path>` resolves `<path>` relative to the project root and produces a chunk holding that file's content, subject to the same per-chunk budget trimming the current file uses.
- The `@file:` string is never passed to keyword search.
- A referenced path that does not exist or cannot be read contributes no chunk; the rest of the retrieval proceeds normally.

Acceptance: A3

Scenario: referenced file appears in context
验收：A3

Given a hint `@file:src/main.rs` for a file that exists in the project
When the provider retrieves context
Then the result contains a chunk whose path ends with `src/main.rs`
And no keyword equal to `@file:src/main.rs` is used in the search

Scenario: missing referenced file is not an error
验收：A3

Given a hint `@file:src/absent.rs` for a file that does not exist
When the provider retrieves context
Then retrieval returns normally, contributing no chunk for that path

## Literal keyword search

Search patterns are text the user typed, not regular expressions the program authored.

- The `ripgrep` invocation matches fixed strings and supplies one pattern per keyword, so several keywords still form a union of matches.
- A keyword containing `+`, `<`, `>`, `(`, `.`, `/`, or any other regex metacharacter matches literally.
- Such a keyword cannot produce a malformed pattern, so it cannot drive the provider into the full-tree fallback scan.
- The existing `rg` behaviors are preserved: honouring `.gitignore`, case-insensitive matching, the timeout, exit code 1 meaning "no match" rather than "failure", and the maximum file count.

Acceptance: A4

Scenario: metacharacter keyword matches literally
验收：A4

Given a keyword containing a regex metacharacter, for example `a+b`
When the provider searches the project
Then files containing the literal text `a+b` are candidates
And files that only match the regex interpretation of `a+b` are not

Scenario: no spurious full-tree fallback
验收：A4

Given a keyword set that would be an invalid regular expression if joined by alternation
When the provider searches with `ripgrep`
Then the search succeeds and returns its match list instead of falling back to the built-in traversal

## Filesystem work off the async worker threads

Directory traversal and the fallback search read the repository from disk. They must not run on a tokio worker thread.

- The synchronous traversal functions perform all `std::fs` work and are not `async`.
- The `async` retrieval path invokes them through `tokio::task::spawn_blocking` and awaits the result.
- Returned content is unchanged: same tree text, same relative paths, same ignored directory names, same depth, entry-count, file-count and visited-node limits.
- A traversal that panics or is cancelled surfaces as an empty or absent result rather than an error thrown into the conversation.

Acceptance: A5

Scenario: tree summary still describes the project
验收：A5

Given a project directory containing `src/main.rs` and an ignored `target/` directory
When the provider builds the tree summary through the blocking pool
Then the summary lists `main.rs` and omits anything under `target`

Scenario: fallback search still finds matches
验收：A5

Given a project where no `ripgrep` is available and one file contains a searched keyword
When the provider runs the built-in fallback traversal
Then that file is returned by its path relative to the project root

## Relevance that reports what matched

A context chunk tells the model why it is there.

- For a keyword search hit, `relevance` names the keywords actually present in that chunk's content.
- For the currently open file, `relevance` marks it as the current file.
- For a file reached through an `@file:` hint, `relevance` marks it as a referenced file.
- When a chunk's content matches none of the keywords, the chunk is still returned but its relevance says so plainly rather than claiming a match.

Acceptance: A7

Scenario: relevance lists matched keywords
验收：A7

Given a retrieved file whose content contains two of the search keywords
When the chunk is produced
Then its `relevance` lists exactly those keywords and does not claim a keyword absent from the content

## Retrieval result for a Chinese question

The combined path — extraction, literal search, chunk assembly — has to end in a retrieved file.

- A question phrased in Chinese whose phrases occur in a repository file yields that file among the retrieved chunks.
- The directory tree summary and the current file are returned even when keyword extraction produces nothing usable.
- Each retrieved chunk respects its own slice of the caller's token budget.

Acceptance: A1

Scenario: Chinese question retrieves the matching file
验收：A1

Given a temporary project whose file content contains a Chinese phrase
And a user message that asks about that phrase in Chinese with no spaces
When the provider retrieves context for that message
Then the retrieved chunks include that file

Scenario: extraction failure degrades to structure only
验收：A1

Given a user message made only of stop-words and single CJK characters
When the provider retrieves context
Then no keyword search runs, the tree summary is still returned, and retrieval reports no error

## Bounded merge across providers

`ContextHub` aggregates several providers, and the caller's budget applies to the aggregate, not to each provider separately.

- Providers are queried in their existing order: built-in first, then plugin providers.
- Chunks are kept in that order while they fit inside `query.max_tokens`; the first chunk that no longer fits, and every later chunk that does not fit the remaining budget, is dropped.
- Kept chunks are never truncated or rewritten by the hub.
- `total_tokens` is recomputed as the sum of the kept chunks' estimates.
- `tree_summary` keeps the first provider that supplied one, and is not counted against the chunk budget.
- When the merged result already fits, every chunk is kept and the result is otherwise identical to before.
- A provider that returns nothing contributes nothing and cannot make the merge fail.

Acceptance: A6

Scenario: over-budget merge is trimmed in provider order
验收：A6

Given two providers whose combined chunks exceed `query.max_tokens`
When the hub retrieves and merges context
Then chunks from the first provider are kept
And later chunks that do not fit the remaining budget are dropped in full
And the reported `total_tokens` equals the sum of the kept chunks' estimates and is within the budget

Scenario: fitting merge is unchanged
验收：A6

Given providers whose combined chunks stay within `query.max_tokens`
When the hub retrieves and merges context
Then every chunk is kept unchanged and the tree summary is taken from the first provider that has one

Scenario: zero-budget request returns nothing rather than everything
验收：A6

Given a query whose `max_tokens` is zero
When the hub merges results from providers that each returned chunks
Then no chunk is kept and the reported total is zero

## Regression expectations

- Existing tests in `xgent_context` keep passing, including tree-summary, fallback-search, truncation, budget and prioritization tests, adjusting an assertion only where it encoded the defect being fixed (for example a keyword length rule expressed in bytes).
- `xgent_agent`'s context formatting is untouched: a chunk still renders as path plus relevance plus content.
- The workspace still compiles and its tests still pass.

Scenario: consumers of the crate still build and test
验收：A8

Given the retrieval changes are implemented
When the crate tests and the workspace check are run
Then `cargo test -p xgent_context` reports no failures
And `cargo check --workspace` reports no errors
