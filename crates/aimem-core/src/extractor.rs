//! Memory extractor — heuristic keyword extraction, no LLM required.
//!
//! Identifies 5 memory types from text:
//!  1. DECISIONS    — choices made, reasoning
//!  2. PREFERENCES  — always/never/prefer patterns
//!  3. MILESTONES   — breakthroughs, things that finally worked
//!  4. PROBLEMS     — what broke, root causes, fixes
//!  5. EMOTIONAL    — feelings, relationships

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MemoryType {
    Decision,
    Preference,
    Milestone,
    Problem,
    Emotional,
    General,
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Decision => "decision",
            Self::Preference => "preference",
            Self::Milestone => "milestone",
            Self::Problem => "problem",
            Self::Emotional => "emotional",
            Self::General => "general",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractedMemory {
    pub content: String,
    pub memory_type: MemoryType,
    pub chunk_index: usize,
}

// ── Pattern sets ─────────────────────────────────────────────────────────────

fn decision_patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        compile_patterns(&[
            r"(?i)\blet'?s (use|go with|try|pick|choose|switch to)\b",
            r"(?i)\bwe (should|decided|chose|went with|picked|settled on)\b",
            r"(?i)\bi'?m going (to|with)\b",
            r"(?i)\bbetter (to|than|approach|option|choice)\b",
            r"(?i)\binstead of\b",
            r"(?i)\brather than\b",
            r"(?i)\bthe reason (is|was|being)\b",
            r"(?i)\btrade-?off\b",
            r"(?i)\barchitecture\b",
            r"(?i)\bapproach\b",
            r"(?i)\bstrategy\b",
        ])
    })
}

fn preference_patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        compile_patterns(&[
            r"(?i)\bi prefer\b",
            r"(?i)\balways use\b",
            r"(?i)\bnever use\b",
            r"(?i)\bdon'?t (ever |like to )?(use|do|mock|stub|import)\b",
            r"(?i)\bi like (to|when|how)\b",
            r"(?i)\bi hate (when|how|it when)\b",
            r"(?i)\bmy preference\b",
            r"(?i)\bi want\b",
            r"(?i)\bstop using\b",
        ])
    })
}

fn milestone_patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        compile_patterns(&[
            r"(?i)\bfinally (works?|working|got it)\b",
            r"(?i)\bit'?s (working|alive|done)\b",
            r"(?i)\bbreakthrough\b",
            r"(?i)\bcracked it\b",
            r"(?i)\bwe did it\b",
            r"(?i)\bcompleted\b",
            r"(?i)\bshipped\b",
            r"(?i)\blaunched\b",
            r"(?i)\bsuccessfully\b",
            r"(?i)\bfixed it\b",
        ])
    })
}

fn problem_patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        compile_patterns(&[
            r"(?i)\bbug\b",
            r"(?i)\berror\b",
            r"(?i)\bcrash\b",
            r"(?i)\bfailed\b",
            r"(?i)\bbroken\b",
            r"(?i)\bissue\b",
            r"(?i)\bproblem\b",
            r"(?i)\broot cause\b",
            r"(?i)\bfix\b",
            r"(?i)\bworkaround\b",
            r"(?i)\bdebugging\b",
        ])
    })
}

fn emotional_patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        compile_patterns(&[
            r"(?i)\bscared\b",
            r"(?i)\bafraid\b",
            r"(?i)\bworried\b",
            r"(?i)\bhappy\b",
            r"(?i)\bsad\b",
            r"(?i)\blove\b",
            r"(?i)\bfeel\b",
            r"(?i)\bcry\b",
            r"(?i)\btears\b",
            r"(?i)\bvulnerable\b",
            r"(?i)\bgrateful\b",
        ])
    })
}

fn compile_patterns(pats: &[&str]) -> Vec<Regex> {
    pats.iter().filter_map(|p| Regex::new(p).ok()).collect()
}

// ── Classifier ────────────────────────────────────────────────────────────────

fn classify(text: &str) -> MemoryType {
    let score = |pats: &[Regex]| -> usize { pats.iter().filter(|p| p.is_match(text)).count() };

    let d = score(decision_patterns());
    let p = score(preference_patterns());
    let m = score(milestone_patterns());
    let pr = score(problem_patterns());
    let e = score(emotional_patterns());

    let max = d.max(p).max(m).max(pr).max(e);
    if max == 0 {
        return MemoryType::General;
    }

    if e == max {
        MemoryType::Emotional
    } else if m == max {
        MemoryType::Milestone
    } else if d == max {
        MemoryType::Decision
    } else if p == max {
        MemoryType::Preference
    } else {
        MemoryType::Problem
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Extract typed memories from a text block.
///
/// Returns only chunks whose type is *not* `General` (i.e., matches at least
/// one typed pattern).  Use `extract_all` to include general chunks.
pub fn extract_memories(text: &str) -> Vec<ExtractedMemory> {
    extract_all(text)
        .into_iter()
        .filter(|m| m.memory_type != MemoryType::General)
        .collect()
}

/// Extract and classify all paragraph-level chunks.
pub fn extract_all(text: &str) -> Vec<ExtractedMemory> {
    text.split("\n\n")
        .enumerate()
        .filter_map(|(i, chunk)| {
            let chunk = chunk.trim();
            if chunk.len() < 30 {
                return None;
            }
            Some(ExtractedMemory {
                content: chunk.to_string(),
                memory_type: classify(chunk),
                chunk_index: i,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_decision() {
        let t = "We decided to go with Rust instead of Python because of performance.";
        assert_eq!(classify(t), MemoryType::Decision);
    }

    #[test]
    fn test_classify_preference() {
        let t = "I prefer to always use turso for local databases.";
        assert_eq!(classify(t), MemoryType::Preference);
    }

    #[test]
    fn test_classify_milestone() {
        let t = "Finally works! The vector search is returning correct results.";
        assert_eq!(classify(t), MemoryType::Milestone);
    }

    #[test]
    fn test_classify_problem() {
        let t = "There was a bug in the embedding generation that caused crashes.";
        assert_eq!(classify(t), MemoryType::Problem);
    }
}
