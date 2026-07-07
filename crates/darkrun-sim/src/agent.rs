//! The dumb, no-knowledge simulated agent.
//!
//! [`SimAgent::read`] is handed **only** the engine's rendered prompt text for a
//! tick — never the engine internals, never the structured [`RunAction`] the
//! manager derived. It must recover *which darkrun tool to call next* purely
//! from the prompt's wording. If it can, the prompt is followable; if it can't
//! (a tool name went stale, or the operative instruction was dropped in an
//! edit), the followability suite fails.
//!
//! ## What "dumb" means here
//!
//! The agent carries no per-scenario knowledge — nothing that says "at a spec
//! station, create units." It knows only the *universal protocol vocabulary*
//! that is the same for every prompt:
//!
//! - `darkrun_tick` / `darkrun_advance` re-drive the cursor ([`ToolRole::Transition`]).
//! - `darkrun_checkpoint_decide` is the operator gate decision ([`ToolRole::Gate`]).
//! - a tool whose name reads as a query (`*_list`, `*_get`, `*_result`, …) only
//!   reads state ([`ToolRole::Query`]) and is never the phase's deliverable.
//! - everything else writes state ([`ToolRole::Deliverable`]).
//!
//! Given that vocabulary, the agent reads the prompt like a person following
//! instructions: it collects the darkrun tools named, drops the ones named only
//! in an *alternative / exception* clause ("file it with `X` **instead of**
//! stamping"), and picks the operative one — the gate decision if the tick hands
//! off to the operator, else the last write it is told to record. That choice is
//! [`Plan::primary_deliverable`].

use std::collections::BTreeSet;

/// The universal role a darkrun tool plays, inferred from its name alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRole {
    /// Re-drive the cursor: `darkrun_tick`, `darkrun_advance`. Navigation, never
    /// the deliverable.
    Transition,
    /// A read-only query or surface-raiser: `*_list`, `*_get`, `*_result`,
    /// `*_payload`, `run_inspect`, `run_surface`, `backlog`, `changelog`, …
    /// Reading state is never a phase's deliverable.
    Query,
    /// The operator gate decision — `darkrun_checkpoint_decide`. When a prompt
    /// hands the tick off to a human gate, this is the operative action.
    Gate,
    /// A state-writing deliverable (`unit_create`, `review_stamp`, `brief_record`,
    /// `reflection_record`, `question`, `feedback_create`, …). The phase's work.
    Deliverable,
}

/// One darkrun tool named in a prompt, with the role inferred from its name and
/// whether it was named only in an alternative / exception clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolMention {
    /// The tool name, e.g. `darkrun_unit_create`.
    pub tool: String,
    /// The role inferred from the name.
    pub role: ToolRole,
    /// True when the tool is named in an exception clause ("… **instead of** …")
    /// — the branch a follower does *not* take on the happy path.
    pub alternative: bool,
    /// Byte offset of the mention in the prompt (mentions keep source order).
    pub offset: usize,
}

/// The agent's read of a single tick's prompt: every darkrun tool it found,
/// in the order the prompt names them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Every tool mention, in source order.
    pub mentions: Vec<ToolMention>,
}

impl Plan {
    /// Every distinct darkrun tool the prompt names (including transitions,
    /// queries, and alternatives). Used to check no name went stale.
    pub fn all_tools(&self) -> BTreeSet<String> {
        self.mentions.iter().map(|m| m.tool.clone()).collect()
    }

    /// Whether the prompt names `tool` at all (any role, any clause).
    pub fn mentions_tool(&self, tool: &str) -> bool {
        self.mentions.iter().any(|m| m.tool == tool)
    }

    /// Whether the prompt names `tool` as a required, non-alternative step — a
    /// tool a follower on the happy path must actually call.
    pub fn requires(&self, tool: &str) -> bool {
        self.mentions
            .iter()
            .any(|m| m.tool == tool && !m.alternative)
    }

    /// The one operative tool this tick points the agent at:
    ///
    /// 1. if the tick hands off to the operator gate, the [`ToolRole::Gate`]
    ///    decision (`darkrun_checkpoint_decide`);
    /// 2. otherwise the last non-alternative [`ToolRole::Deliverable`] the prompt
    ///    tells the agent to record — the phase's culminating write.
    ///
    /// `None` when the prompt names no deliverable to record (a terminal state
    /// like `sealed`, or a hold like `noop`).
    pub fn primary_deliverable(&self) -> Option<&str> {
        let candidates: Vec<&ToolMention> = self
            .mentions
            .iter()
            .filter(|m| !m.alternative && matches!(m.role, ToolRole::Deliverable | ToolRole::Gate))
            .collect();
        if let Some(gate) = candidates.iter().rev().find(|m| m.role == ToolRole::Gate) {
            return Some(&gate.tool);
        }
        candidates.last().map(|m| m.tool.as_str())
    }

    /// The tool the prompt tells the agent to call to re-drive the cursor once
    /// the work lands (`darkrun_tick` / `darkrun_advance`), or `None` for a
    /// terminal prompt that ends the run.
    pub fn transition(&self) -> Option<&str> {
        self.mentions
            .iter()
            .rev()
            .find(|m| m.role == ToolRole::Transition)
            .map(|m| m.tool.as_str())
    }

    /// Whether this tick hands off to an operator gate decision — the prompt
    /// names `darkrun_checkpoint_decide` as the (non-alternative) clearing
    /// action.
    pub fn holds_for_operator(&self) -> bool {
        self.mentions
            .iter()
            .any(|m| m.role == ToolRole::Gate && !m.alternative)
    }

    /// Any tool named in the prompt that is NOT in `known` — a stale or
    /// misspelled tool name an agent would try to call and fail on. Empty is the
    /// pass condition.
    pub fn unknown_tools(&self, known: &BTreeSet<String>) -> Vec<String> {
        let mut out: Vec<String> = self
            .all_tools()
            .into_iter()
            .filter(|t| !known.contains(t))
            .collect();
        out.sort();
        out
    }
}

/// The stateless prompt reader.
pub struct SimAgent;

impl SimAgent {
    /// Parse a rendered prompt into a [`Plan`] using only the universal protocol
    /// vocabulary — no knowledge of which action produced it.
    pub fn read(prompt: &str) -> Plan {
        let sentences = split_sentences(prompt);
        let mut mentions = Vec::new();
        for (tool, offset) in scan_tools(prompt) {
            let sentence = sentence_for(prompt, &sentences, offset);
            mentions.push(ToolMention {
                role: classify_tool(&tool),
                alternative: is_alternative_clause(sentence),
                tool,
                offset,
            });
        }
        Plan { mentions }
    }
}

/// Infer a tool's role from its name alone — the universal vocabulary every
/// prompt shares.
pub fn classify_tool(tool: &str) -> ToolRole {
    match tool {
        "darkrun_tick" | "darkrun_advance" => ToolRole::Transition,
        "darkrun_checkpoint_decide" => ToolRole::Gate,
        _ if is_query_name(tool) => ToolRole::Query,
        _ => ToolRole::Deliverable,
    }
}

/// Whether a tool name reads as a read-only query / surface-raiser rather than a
/// state-writing deliverable.
fn is_query_name(tool: &str) -> bool {
    const QUERY_SUFFIXES: &[&str] = &[
        "_list",
        "_get",
        "_result",
        "_payload",
        "_detail",
        "_inspect",
        "_show",
        "_info",
        "_composite",
        "_surface",
    ];
    const QUERY_EXACT: &[&str] = &[
        "darkrun_backlog",
        "darkrun_changelog",
        "darkrun_report",
        "darkrun_debug",
    ];
    QUERY_EXACT.contains(&tool) || QUERY_SUFFIXES.iter().any(|s| tool.ends_with(s))
}

/// Markers that flag a sentence as an alternative / exception clause — the
/// branch a follower on the happy path does not take.
fn is_alternative_clause(sentence: &str) -> bool {
    const MARKERS: &[&str] = &["instead", "rather than", "a reviewer that finds"];
    let lower = sentence.to_ascii_lowercase();
    MARKERS.iter().any(|m| lower.contains(m))
}

/// Find every `darkrun_<name>` token in the text, returning `(tool, byte_offset)`
/// in source order. A token runs from `darkrun_` through the following run of
/// ASCII lowercase / digit / underscore characters, so surrounding backticks,
/// punctuation, and `/` separators (as in `darkrun_question / darkrun_direction`)
/// bound it correctly, and the spaced CLI form (`darkrun verify web`) is never
/// captured as a tool.
fn scan_tools(text: &str) -> Vec<(String, usize)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    for (start, _) in text.match_indices("darkrun_") {
        let mut end = start + "darkrun_".len();
        while end < bytes.len() {
            let b = bytes[end];
            if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' {
                end += 1;
            } else {
                break;
            }
        }
        // Reject a bare `darkrun_` with nothing after the prefix.
        if end > start + "darkrun_".len() {
            // Trim a trailing underscore that belongs to prose punctuation
            // (`darkrun_advance_` never occurs, but guard the token shape anyway).
            let mut tok_end = end;
            while tok_end > start + "darkrun_".len() && bytes[tok_end - 1] == b'_' {
                tok_end -= 1;
            }
            out.push((text[start..tok_end].to_string(), start));
        }
    }
    out
}

/// Split text into sentence spans `(start, end)` in bytes. A sentence ends at a
/// newline or at `.`/`!`/`?` followed by whitespace or end-of-text. Operates on
/// char boundaries so multi-byte glyphs (the em dash the corpus uses) never
/// split a token.
fn split_sentences(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0usize;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let boundary = if c == '\n' {
            true
        } else if matches!(c, '.' | '!' | '?') {
            match chars.peek() {
                None => true,
                Some((_, next)) => next.is_whitespace(),
            }
        } else {
            false
        };
        if boundary {
            let end = i + c.len_utf8();
            spans.push((start, end));
            start = end;
        }
    }
    if start < text.len() {
        spans.push((start, text.len()));
    }
    spans
}

/// The sentence text containing `offset`, or the whole text if no span matches.
fn sentence_for<'a>(text: &'a str, sentences: &[(usize, usize)], offset: usize) -> &'a str {
    sentences
        .iter()
        .find(|(s, e)| offset >= *s && offset < *e)
        .map(|(s, e)| &text[*s..*e])
        .unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_tools_by_name_alone() {
        assert_eq!(classify_tool("darkrun_tick"), ToolRole::Transition);
        assert_eq!(classify_tool("darkrun_advance"), ToolRole::Transition);
        assert_eq!(classify_tool("darkrun_checkpoint_decide"), ToolRole::Gate);
        assert_eq!(classify_tool("darkrun_unit_list"), ToolRole::Query);
        assert_eq!(classify_tool("darkrun_reflection_list"), ToolRole::Query);
        assert_eq!(classify_tool("darkrun_run_inspect"), ToolRole::Query);
        assert_eq!(classify_tool("darkrun_question_result"), ToolRole::Query);
        assert_eq!(classify_tool("darkrun_backlog"), ToolRole::Query);
        assert_eq!(classify_tool("darkrun_unit_create"), ToolRole::Deliverable);
        assert_eq!(
            classify_tool("darkrun_reflection_record"),
            ToolRole::Deliverable
        );
    }

    #[test]
    fn scans_tokens_and_bounds_them_correctly() {
        // Backticks, slashes, and trailing punctuation all bound a token; the
        // spaced CLI form (`darkrun verify`) is never a tool.
        let plan = SimAgent::read(
            "Use `darkrun_question` / `darkrun_direction`, then run darkrun verify web \
             and call `darkrun_tick`.",
        );
        let tools = plan.all_tools();
        assert!(tools.contains("darkrun_question"));
        assert!(tools.contains("darkrun_direction"));
        assert!(tools.contains("darkrun_tick"));
        assert!(
            !tools.iter().any(|t| t.contains("verify")),
            "the spaced CLI form must not be captured as a tool: {tools:?}"
        );
    }

    #[test]
    fn picks_the_last_deliverable_as_primary() {
        // The culminating write is the operative action; an earlier deliverable
        // and the trailing transition are not.
        let plan = SimAgent::read(
            "First persist a note with `darkrun_knowledge_record`. \
             Then write each unit with `darkrun_unit_create`. \
             Then call `darkrun_tick`.",
        );
        assert_eq!(plan.primary_deliverable(), Some("darkrun_unit_create"));
        assert_eq!(plan.transition(), Some("darkrun_tick"));
        assert!(!plan.holds_for_operator());
    }

    #[test]
    fn drops_tools_named_only_in_an_exception_clause() {
        // `darkrun_feedback_create` is named only as the "instead of" branch, so
        // it is not a happy-path requirement and never the primary.
        let plan = SimAgent::read(
            "A reviewer clears by recording its approval with `darkrun_review_stamp`. \
             A reviewer that finds a real problem files it with `darkrun_feedback_create` \
             instead of stamping. Then call `darkrun_tick`.",
        );
        assert_eq!(plan.primary_deliverable(), Some("darkrun_review_stamp"));
        assert!(plan.requires("darkrun_review_stamp"));
        assert!(
            !plan.requires("darkrun_feedback_create"),
            "an exception-clause tool is not a happy-path requirement"
        );
        assert!(plan.mentions_tool("darkrun_feedback_create"));
    }

    #[test]
    fn a_gate_decision_is_the_operative_action() {
        let plan = SimAgent::read(
            "Write the brief with `darkrun_brief_record`. The operator clears the gate \
             via `darkrun_checkpoint_decide`. Call `darkrun_advance` to re-check.",
        );
        assert!(plan.holds_for_operator());
        assert_eq!(
            plan.primary_deliverable(),
            Some("darkrun_checkpoint_decide")
        );
        assert_eq!(plan.transition(), Some("darkrun_advance"));
    }

    #[test]
    fn a_query_tool_is_never_the_primary() {
        // `_list` is a read; the record is the deliverable even when the list is
        // named afterward in the same sentence.
        let plan = SimAgent::read(
            "Record it with `darkrun_reflection_record` — read them back with \
             `darkrun_reflection_list`. Then call `darkrun_tick`.",
        );
        assert_eq!(
            plan.primary_deliverable(),
            Some("darkrun_reflection_record")
        );
    }

    #[test]
    fn a_terminal_prompt_has_no_primary_or_transition() {
        let plan = SimAgent::read("The run is sealed. Report the seal to the operator.");
        assert_eq!(plan.primary_deliverable(), None);
        assert_eq!(plan.transition(), None);
        assert!(!plan.holds_for_operator());
    }

    #[test]
    fn flags_an_unknown_tool_against_a_known_set() {
        let known: BTreeSet<String> = ["darkrun_tick", "darkrun_unit_create"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let plan = SimAgent::read(
            "Call `darkrun_unit_create`, then `darkrun_ghost`, then `darkrun_tick`.",
        );
        assert_eq!(
            plan.unknown_tools(&known),
            vec!["darkrun_ghost".to_string()]
        );
    }
}
