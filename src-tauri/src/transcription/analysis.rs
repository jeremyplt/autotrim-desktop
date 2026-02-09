use serde::{Deserialize, Serialize};
use anyhow::{Result, Context};
use crate::ffmpeg::Segment;

use super::Word;

#[derive(Debug, Clone)]
pub struct Phrase {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub words: Vec<Word>,
}

pub fn segment_into_phrases(words: &[Word]) -> Vec<Phrase> {
    let mut phrases = Vec::new();
    let mut current_phrase_words: Vec<Word> = Vec::new();
    let mut current_text = String::new();

    for (idx, word) in words.iter().enumerate() {
        current_phrase_words.push(word.clone());
        current_text.push_str(&word.word);
        current_text.push(' ');

        let ends_with_punctuation = word.word.trim_end().ends_with(|c: char| {
            c == '.' || c == '!' || c == '?' || c == ','
        });

        // Check if there's a pause > 0.4s after this word (natural phrase boundary)
        let has_pause = if idx + 1 < words.len() {
            words[idx + 1].start - word.end > 0.4
        } else {
            false
        };

        let should_split = ends_with_punctuation || has_pause;

        if should_split && !current_phrase_words.is_empty() {
            let start = current_phrase_words.first().unwrap().start;
            let end = current_phrase_words.last().unwrap().end;

            phrases.push(Phrase {
                text: current_text.trim().to_string(),
                start,
                end,
                words: current_phrase_words.clone(),
            });

            current_phrase_words.clear();
            current_text.clear();
        }
    }

    // Add remaining words as a phrase
    if !current_phrase_words.is_empty() {
        let start = current_phrase_words.first().unwrap().start;
        let end = current_phrase_words.last().unwrap().end;

        phrases.push(Phrase {
            text: current_text.trim().to_string(),
            start,
            end,
            words: current_phrase_words,
        });
    }

    phrases
}

/// Filler words that should be ignored for silence gap detection.
/// These are mouth noises / hesitations that Whisper transcribes as words,
/// breaking silence gaps into smaller sub-threshold pieces.
const FILLER_WORDS: &[&str] = &[
    "euh", "hum", "um", "uh", "ah", "oh", "hein", "bah", "ben",
    "hmm", "hm", "mhm", "ouais", "eh",
];

pub fn is_filler_word(word: &str) -> bool {
    let normalized = word.to_lowercase();
    let cleaned: String = normalized.chars().filter(|c| c.is_alphanumeric()).collect();
    FILLER_WORDS.contains(&cleaned.as_str())
}

/// Filter out filler words from the word list for silence detection purposes.
/// Returns a new list with fillers removed (their time gaps merge with surrounding silence).
pub fn filter_filler_words(words: &[Word]) -> Vec<Word> {
    words.iter()
        .filter(|w| !is_filler_word(&w.word))
        .cloned()
        .collect()
}

/// Contextual filler word filtering: only removes fillers that are "isolated"
/// (significant gap before OR after), keeping fillers embedded in flowing speech.
pub fn filter_filler_words_contextual(words: &[Word], context_gap: f64) -> Vec<Word> {
    words.iter()
        .enumerate()
        .filter(|(i, w)| {
            if !is_filler_word(&w.word) {
                return true; // Always keep non-filler words
            }

            // Measure gap before this filler
            let gap_before = if *i > 0 {
                w.start - words[*i - 1].end
            } else {
                f64::MAX // First word in sequence = isolated
            };

            // Measure gap after this filler
            let gap_after = if *i < words.len() - 1 {
                words[*i + 1].start - w.end
            } else {
                f64::MAX // Last word in sequence = isolated
            };

            // Keep filler only if embedded in flowing speech (both gaps small)
            gap_before < context_gap && gap_after < context_gap
        })
        .map(|(_, w)| w.clone())
        .collect()
}

// --- Passage-based segmentation and retake detection via Claude ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Passage {
    pub id: usize,
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub word_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetakeGroup {
    pub group_id: usize,
    pub description: String,
    pub passages: Vec<usize>,
    pub keep: Vec<usize>,
    pub remove: Vec<usize>,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbandonedPassage {
    pub id: usize,
    pub reason: String,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetakeDetectionResult {
    pub retake_groups: Vec<RetakeGroup>,
    pub abandoned_passages: Vec<AbandonedPassage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupVerification {
    pub group_id: usize,
    pub approved: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub verified_groups: Vec<GroupVerification>,
}

// Anthropic API response structures
#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { #[allow(dead_code)] text: String },
    #[serde(rename = "tool_use")]
    ToolUse { #[allow(dead_code)] id: String, #[allow(dead_code)] name: String, input: serde_json::Value },
    #[serde(rename = "thinking")]
    Thinking { #[allow(dead_code)] thinking: String },
    #[serde(other)]
    Unknown,
}

/// Segment words into passages for retake detection, skipping words inside removed segments.
pub fn segment_into_passages(words: &[Word], segments_to_skip: &[Segment]) -> Vec<Passage> {
    // Filter out words that fall inside segments to skip
    let meaningful_words: Vec<&Word> = words.iter()
        .filter(|w| {
            !segments_to_skip.iter().any(|seg| w.start >= seg.start && w.end <= seg.end)
        })
        .collect();

    if meaningful_words.is_empty() {
        return Vec::new();
    }

    let mut passages = Vec::new();
    let mut current_words: Vec<&Word> = Vec::new();
    let mut current_text = String::new();
    let mut current_start = meaningful_words[0].start;
    let mut best_gap_in_passage: Option<(usize, f64)> = None;

    for (idx, word) in meaningful_words.iter().enumerate() {
        if current_words.is_empty() {
            current_start = word.start;
            best_gap_in_passage = None;
        }

        current_words.push(word);
        current_text.push_str(&word.word);
        current_text.push(' ');

        let word_count = current_words.len();

        let gap_after = if idx + 1 < meaningful_words.len() {
            meaningful_words[idx + 1].start - word.end
        } else {
            999.0
        };

        if word_count >= 3 && gap_after < 999.0 {
            if let Some((_, best_gap)) = best_gap_in_passage {
                if gap_after > best_gap {
                    best_gap_in_passage = Some((word_count - 1, gap_after));
                }
            } else {
                best_gap_in_passage = Some((word_count - 1, gap_after));
            }
        }

        let restart_match_len = 3;
        let is_restart = word_count >= restart_match_len
            && idx + restart_match_len < meaningful_words.len()
            && {
                let normalize = |s: &str| -> String {
                    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
                };
                let opener: Vec<String> = current_words[..restart_match_len].iter()
                    .map(|w| normalize(&w.word))
                    .collect();
                let upcoming: Vec<String> = (1..=restart_match_len)
                    .filter_map(|k| meaningful_words.get(idx + k))
                    .map(|w| normalize(&w.word))
                    .collect();
                upcoming.len() == restart_match_len && opener == upcoming
            };

        if is_restart {
            if word_count >= 3 {
                passages.push(Passage {
                    id: passages.len(),
                    text: current_text.trim().to_string(),
                    start: current_start,
                    end: word.end,
                    word_count,
                });
            }
            current_words.clear();
            current_text.clear();
            best_gap_in_passage = None;
            continue;
        }

        let should_split =
            gap_after >= 0.7
            || (word_count >= 30 && gap_after >= 0.3)
            || idx == meaningful_words.len() - 1;

        let force_split_large = word_count >= 60 && !should_split;

        if force_split_large {
            if let Some((split_at, _)) = best_gap_in_passage {
                let split_words: Vec<&Word> = current_words[..=split_at].to_vec();
                let split_text: String = split_words.iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" ");
                let split_end = split_words.last().unwrap().end;
                passages.push(Passage {
                    id: passages.len(),
                    text: split_text,
                    start: current_start,
                    end: split_end,
                    word_count: split_words.len(),
                });
                let remaining: Vec<&Word> = current_words[split_at + 1..].to_vec();
                current_words = remaining;
                current_text = current_words.iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" ");
                current_text.push(' ');
                current_start = current_words.first().map(|w| w.start).unwrap_or(word.end);
                best_gap_in_passage = None;
            }
            continue;
        }

        let too_short = word_count < 3;
        let forced = idx == meaningful_words.len() - 1 || gap_after >= 5.0;

        if should_split && (!too_short || forced) {
            passages.push(Passage {
                id: passages.len(),
                text: current_text.trim().to_string(),
                start: current_start,
                end: word.end,
                word_count,
            });
            current_words.clear();
            current_text.clear();
            best_gap_in_passage = None;
        }
    }

    passages
}

/// Detect obvious false starts: consecutive passages with the same opening words
pub fn detect_false_starts(passages: &[Passage]) -> Vec<usize> {
    let mut false_start_ids = Vec::new();
    let min_match = 3;

    let normalize = |s: &str| -> String {
        s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
    };

    for i in 0..passages.len() {
        if false_start_ids.contains(&passages[i].id) {
            continue;
        }

        let curr_words: Vec<String> = passages[i].text.split_whitespace()
            .map(|w| normalize(w))
            .collect();

        if curr_words.len() < min_match {
            continue;
        }

        let curr_opener = &curr_words[..min_match];

        for j in (i + 1)..passages.len() {
            if passages[j].start - passages[i].end > 120.0 {
                break;
            }

            let next_words: Vec<String> = passages[j].text.split_whitespace()
                .map(|w| normalize(w))
                .collect();

            if next_words.len() < min_match {
                continue;
            }

            let next_opener = &next_words[..min_match];

            if curr_opener == next_opener && passages[i].word_count < passages[j].word_count {
                let curr_rest: Vec<&String> = curr_words[min_match..].iter().collect();

                if curr_rest.is_empty() {
                    false_start_ids.push(passages[i].id);
                    break;
                }

                let next_rest_set: std::collections::HashSet<&String> =
                    next_words[min_match..].iter().collect();
                let overlap = curr_rest.iter()
                    .filter(|w| next_rest_set.contains(*w))
                    .count();
                let overlap_ratio = overlap as f64 / curr_rest.len() as f64;

                if overlap_ratio >= 0.7 {
                    false_start_ids.push(passages[i].id);
                    break;
                }
            }
        }
    }

    false_start_ids
}

/// Detect retake sequences: groups of 3+ passages sharing the same 3-word opener
pub fn detect_retake_sequences(passages: &[Passage], already_removed: &[usize]) -> Vec<usize> {
    use std::collections::HashMap;

    let min_match = 3;
    let min_group_size = 3;
    let max_time_span = 180.0;

    let normalize = |s: &str| -> String {
        s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
    };

    let removed_set: std::collections::HashSet<usize> = already_removed.iter().copied().collect();

    let mut opener_groups: HashMap<Vec<String>, Vec<usize>> = HashMap::new();

    for p in passages {
        if removed_set.contains(&p.id) {
            continue;
        }

        let words: Vec<String> = p.text.split_whitespace()
            .take(min_match)
            .map(|w| normalize(w))
            .collect();

        if words.len() == min_match {
            opener_groups.entry(words).or_default().push(p.id);
        }
    }

    let mut to_remove = Vec::new();

    for (opener, ids) in &opener_groups {
        if ids.len() < min_group_size {
            continue;
        }

        let first_passage = passages.get(*ids.first().unwrap());
        let last_passage = passages.get(*ids.last().unwrap());

        if let (Some(first), Some(last)) = (first_passage, last_passage) {
            let time_span = last.end - first.start;
            if time_span > max_time_span {
                continue;
            }

            let keep_id = *ids.last().unwrap();

            eprintln!("Retake sequence ({} passages, {:.0}s span): opener=\"{}\", keep=[{}], remove={:?}",
                ids.len(), time_span,
                opener.join(" "),
                keep_id,
                ids.iter().filter(|&&id| id != keep_id).collect::<Vec<_>>());

            for &id in ids {
                if id != keep_id {
                    to_remove.push(id);
                }
            }
        }
    }

    to_remove
}

// --- Simplified pipeline: chunk-based AI analysis ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechChunk {
    pub id: usize,
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub word_count: usize,
}

/// Segment words into speech chunks for AI analysis.
/// Splits on pauses > gap_threshold and on within-chunk restarts.
pub fn segment_into_chunks(words: &[Word], gap_threshold: f64) -> Vec<SpeechChunk> {
    if words.is_empty() {
        return Vec::new();
    }

    let normalize = |s: &str| -> String {
        s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
    };

    let mut chunks = Vec::new();
    let mut current_words: Vec<&Word> = Vec::new();
    let mut current_text = String::new();

    for (idx, word) in words.iter().enumerate() {
        if current_words.is_empty() {
            // Starting a new chunk
        }

        current_words.push(word);
        current_text.push_str(&word.word);
        current_text.push(' ');

        let word_count = current_words.len();

        // Gap to next word
        let gap_after = if idx + 1 < words.len() {
            words[idx + 1].start - word.end
        } else {
            999.0 // force split at end
        };

        // Detect within-chunk restart: next 3 words match ANY 3-word window in the current chunk
        let restart_match_len = 3;
        let is_restart = word_count >= restart_match_len
            && idx + restart_match_len < words.len()
            && {
                let upcoming: Vec<String> = (1..=restart_match_len)
                    .filter_map(|k| words.get(idx + k))
                    .map(|w| normalize(&w.word))
                    .collect();
                upcoming.len() == restart_match_len
                    && current_words.windows(restart_match_len).any(|window| {
                        let window_norm: Vec<String> = window.iter()
                            .map(|w| normalize(&w.word))
                            .collect();
                        window_norm == upcoming
                    })
            };

        let should_split = gap_after >= gap_threshold
            || is_restart
            || idx == words.len() - 1;

        if should_split && !current_words.is_empty() {
            let start = current_words.first().unwrap().start;
            let end = current_words.last().unwrap().end;
            chunks.push(SpeechChunk {
                id: chunks.len(),
                text: current_text.trim().to_string(),
                start,
                end,
                word_count: current_words.len(),
            });
            current_words.clear();
            current_text.clear();
        }
    }

    chunks
}

/// French stop words for content-word extraction.
/// Used to filter out common words when computing semantic overlap between chunks.
const FRENCH_STOP_WORDS: &[&str] = &[
    "le", "la", "les", "un", "une", "des", "de", "du", "au", "aux",
    "ce", "cette", "ces", "mon", "ma", "mes", "ton", "ta", "tes",
    "son", "sa", "ses", "notre", "votre", "leur", "leurs",
    "je", "tu", "il", "elle", "on", "nous", "vous", "ils", "elles",
    "me", "te", "se", "lui", "en", "y", "ca",
    "et", "ou", "mais", "donc", "car", "ni", "que", "qui", "quoi",
    "ne", "pas", "plus", "tres", "aussi", "tout", "comme",
    "est", "a", "sont", "ont", "fait", "va", "etre", "avoir", "jai",
    "dans", "sur", "avec", "pour", "par", "sans", "chez",
    "si", "quand", "cest", "il", "ya", "bon", "oui", "non",
    "puis", "encore", "deja", "peu", "petit", "ici", "la",
    "moi", "toi", "soi", "eux",
    "voila", "hein", "bah", "ben", "ouais",
];

/// Normalize a word for comparison: lowercase, keep only alphanumeric chars.
fn normalize_word(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Extract content words from a text (non-stop-words, normalized).
fn extract_content_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| normalize_word(w))
        .filter(|w| w.len() >= 3 && !FRENCH_STOP_WORDS.contains(&w.as_str()))
        .collect()
}

/// Extract all 3-word windows (normalized) from a chunk's text.
fn extract_ngram_windows(text: &str, n: usize) -> Vec<Vec<String>> {
    let words: Vec<String> = text.split_whitespace()
        .map(|w| normalize_word(w))
        .collect();
    if words.len() < n {
        return Vec::new();
    }
    words.windows(n).map(|w| w.to_vec()).collect()
}

/// Check if two chunks share any N-gram window.
pub fn has_ngram_overlap(text_a: &str, text_b: &str, n: usize) -> bool {
    let windows_a = extract_ngram_windows(text_a, n);
    let windows_b: std::collections::HashSet<Vec<String>> =
        extract_ngram_windows(text_b, n).into_iter().collect();
    windows_a.iter().any(|w| windows_b.contains(w))
}

/// Get the shared N-grams between two texts (for reporting).
fn shared_ngrams(text_a: &str, text_b: &str, n: usize) -> Vec<Vec<String>> {
    let windows_a = extract_ngram_windows(text_a, n);
    let windows_b: std::collections::HashSet<Vec<String>> =
        extract_ngram_windows(text_b, n).into_iter().collect();
    let mut shared: Vec<Vec<String>> = windows_a.into_iter()
        .filter(|w| windows_b.contains(w))
        .collect();
    shared.sort();
    shared.dedup();
    shared
}

/// Count shared content words between two texts.
pub fn count_shared_content_words(text_a: &str, text_b: &str) -> usize {
    let words_a: std::collections::HashSet<String> = extract_content_words(text_a).into_iter().collect();
    let words_b: std::collections::HashSet<String> = extract_content_words(text_b).into_iter().collect();
    words_a.intersection(&words_b).count()
}

/// Check if a chunk's text appears truncated (aborted sentence).
fn is_truncated(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.ends_with('—')
        || trimmed.ends_with("--")
        || trimmed.ends_with("...")
        || trimmed.ends_with("…")
}

/// Detect groups of chunks that are retakes via multiple strategies:
/// 1. Same 3-word opener (original logic, lowered to min_group_size=2)
/// 2. Cross-chunk N-gram matching (any shared 3-word sequence)
/// 3. Content-word overlap (for truly different reformulations)
fn build_retake_hints(chunks: &[SpeechChunk]) -> String {
    use std::collections::{HashMap, HashSet};

    let min_match = 3;
    let max_time_span = 180.0; // Allow wider retake groups — speaker can retry for 3 min
    let max_gap_between_members = 60.0; // Allow 60s gap between members (retakes can have long pauses)
    let max_opener_frequency = 6; // Skip openers appearing in 7+ chunks (too common)

    // Track all retake pairs we've found (earlier_id, later_id) to avoid duplicates
    let mut retake_pairs: Vec<(usize, usize, String)> = Vec::new(); // (remove_id, keep_id, reason)

    // === TIER 1: Same 3-word opener (min_group_size=2 with content overlap) ===
    let mut opener_groups: HashMap<Vec<String>, Vec<usize>> = HashMap::new();
    for chunk in chunks {
        if chunk.word_count < min_match {
            continue;
        }
        let words: Vec<String> = chunk.text.split_whitespace()
            .take(min_match)
            .map(|w| normalize_word(w))
            .collect();
        if words.len() == min_match {
            opener_groups.entry(words).or_default().push(chunk.id);
        }
    }

    let mut opener_hint_ids: HashSet<usize> = HashSet::new();
    let mut hints = Vec::new();

    for (opener_key, ids) in &opener_groups {
        if ids.len() < 2 {
            continue;
        }

        // Skip very common openers — they're French transitions, not retake signals
        if ids.len() > max_opener_frequency {
            eprintln!("Retake hints: skipping common opener '{}' ({} chunks)", opener_key.join(" "), ids.len());
            continue;
        }

        // Split into sub-groups based on max_gap_between_members.
        // This prevents a single large gap from invalidating the entire group.
        let mut sub_groups: Vec<Vec<usize>> = Vec::new();
        let mut current_sub: Vec<usize> = vec![ids[0]];

        for pair in ids.windows(2) {
            if let (Some(a), Some(b)) = (chunks.get(pair[0]), chunks.get(pair[1])) {
                if b.start - a.end > max_gap_between_members {
                    // Gap too large, start a new sub-group
                    sub_groups.push(current_sub);
                    current_sub = vec![pair[1]];
                } else {
                    current_sub.push(pair[1]);
                }
            }
        }
        sub_groups.push(current_sub);

        for sub_ids in &sub_groups {
            if sub_ids.len() < 2 {
                continue;
            }

            // Check time span within sub-group
            let first = chunks.get(*sub_ids.first().unwrap());
            let last = chunks.get(*sub_ids.last().unwrap());
            if let (Some(f), Some(l)) = (first, last) {
                if l.end - f.start > max_time_span {
                    continue;
                }
            }

            // Require content overlap for ALL group sizes (not just pairs).
            // Common French openers match across unrelated content — content overlap
            // confirms they're actually discussing the same topic.
            {
                // Check pairwise: at least one pair in the group must share ≥3 content words
                let mut has_overlap = false;
                'outer: for (ai, &id_a) in sub_ids.iter().enumerate() {
                    for &id_b in &sub_ids[ai + 1..] {
                        if let (Some(a), Some(b)) = (chunks.get(id_a), chunks.get(id_b)) {
                            let shared = count_shared_content_words(&a.text, &b.text);
                            if shared >= 3 {
                                has_overlap = true;
                                break 'outer;
                            }
                        }
                    }
                }
                if !has_overlap {
                    continue;
                }
            }

            let last_id = *sub_ids.last().unwrap();
            let opener_text = opener_key.join(" ");

            let mut detail_lines = Vec::new();
            for &id in sub_ids {
                if let Some(c) = chunks.get(id) {
                    let preview: String = c.text.chars().take(60).collect();
                    let ellipsis = if c.text.chars().count() > 60 { "..." } else { "" };
                    let marker = if id == last_id { " ← GARDER" } else { " ← supprimer" };
                    detail_lines.push(format!(
                        "  [{}] ({} mots) \"{}{}\"{}", id, c.word_count, preview, ellipsis, marker
                    ));
                    opener_hint_ids.insert(id);
                }
            }

            hints.push(format!(
                "⚠️ REPRISES DÉTECTÉES (ouverture \"{}\"): segments {:?} → garder SEULEMENT [{}]\n{}",
                opener_text, sub_ids, last_id, detail_lines.join("\n")
            ));

            for &id in sub_ids {
                if id != last_id {
                    retake_pairs.push((id, last_id, format!("opener \"{}\"", opener_text)));
                }
            }
        }
    }

    // === TIER 1.5: Post-keep continuation detection ===
    // After finding a group [A,B,C] → keep C, check if chunks AFTER C also share
    // content with the group. This catches "continued retake sequences" where the
    // speaker makes MORE attempts after what seemed like the final version.
    {
        let mut extended_pairs: Vec<(usize, usize, String)> = Vec::new();

        // Group retake_pairs by keep_id to find the current "keep" for each group
        let mut keep_to_group: HashMap<usize, Vec<usize>> = HashMap::new();
        for &(remove_id, keep_id, _) in &retake_pairs {
            keep_to_group.entry(keep_id).or_default().push(remove_id);
        }

        for (&old_keep_id, _group_members) in &keep_to_group {
            let old_keep = match chunks.get(old_keep_id) {
                Some(c) => c,
                None => continue,
            };

            // Look at chunks after old_keep_id, within 60s
            let mut new_last_id = old_keep_id;
            for candidate_id in (old_keep_id + 1)..chunks.len() {
                let candidate = match chunks.get(candidate_id) {
                    Some(c) => c,
                    None => break,
                };

                let gap = candidate.start - old_keep.end;
                if gap > 60.0 {
                    break;
                }

                // Check if candidate shares content with the old keep
                let shared = count_shared_content_words(&old_keep.text, &candidate.text);
                let candidate_opener: Vec<String> = candidate.text.split_whitespace()
                    .take(min_match)
                    .map(|w| normalize_word(w))
                    .collect();
                let keep_opener: Vec<String> = old_keep.text.split_whitespace()
                    .take(min_match)
                    .map(|w| normalize_word(w))
                    .collect();
                let same_opener = candidate_opener.len() == min_match
                    && keep_opener.len() == min_match
                    && candidate_opener == keep_opener;

                // Strong signal: same opener OR high content overlap
                if same_opener || shared >= 3 {
                    // This is a continuation of the retake sequence
                    if candidate_id > new_last_id {
                        new_last_id = candidate_id;
                    }
                }
            }

            if new_last_id > old_keep_id {
                // The old keep becomes a remove, new_last_id becomes the new keep
                extended_pairs.push((old_keep_id, new_last_id,
                    format!("extended retake: old keep [{}] superseded by [{}]", old_keep_id, new_last_id)));

                // Also mark intermediate chunks as removes
                for between_id in (old_keep_id + 1)..new_last_id {
                    if let Some(between) = chunks.get(between_id) {
                        let shared_with_new = chunks.get(new_last_id)
                            .map(|k| count_shared_content_words(&between.text, &k.text))
                            .unwrap_or(0);
                        // Only remove if it's related (shared content or fragment)
                        if shared_with_new >= 2 || between.word_count < 8 {
                            extended_pairs.push((between_id, new_last_id,
                                format!("part of extended retake [{}-{}]", old_keep_id, new_last_id)));
                        }
                    }
                }

                if let Some(new_keep) = chunks.get(new_last_id) {
                    let preview: String = new_keep.text.chars().take(60).collect();
                    let ellipsis = if new_keep.text.chars().count() > 60 { "..." } else { "" };
                    hints.push(format!(
                        "⚠️ REPRISES PROLONGÉES: le groupe se poursuit après [{}] → garder [{}] \"{}{}\"\n  Les segments [{}-{}] sont des tentatives supplémentaires → supprimer",
                        old_keep_id, new_last_id, preview, ellipsis, old_keep_id, new_last_id - 1
                    ));
                }

                eprintln!("Retake hints: extended group past [{}] to [{}]", old_keep_id, new_last_id);
            }
        }

        retake_pairs.extend(extended_pairs);
    }

    // === TIER 2: Cross-chunk N-gram matching ===
    // For pairs within 30s, check if any 3-word sequence from one appears in the other.
    // Only match N-grams containing at least one meaningful word (not all stop words).

    // Count N-gram frequency across all chunks to filter out very common ones
    let mut ngram_frequency: HashMap<Vec<String>, usize> = HashMap::new();
    for chunk in chunks {
        let windows: HashSet<Vec<String>> = extract_ngram_windows(&chunk.text, 3).into_iter().collect();
        for w in windows {
            *ngram_frequency.entry(w).or_insert(0) += 1;
        }
    }
    let max_ngram_freq = 4; // Ignore N-grams appearing in 4+ chunks (too common)

    for i in 0..chunks.len() {
        for j in (i + 1)..chunks.len() {
            // Both already handled by opener hints? Skip.
            if opener_hint_ids.contains(&chunks[i].id) && opener_hint_ids.contains(&chunks[j].id) {
                continue;
            }

            let gap = chunks[j].start - chunks[i].end;
            if gap > 30.0 || gap < 0.0 {
                continue;
            }

            // Skip very short chunks (< 5 words: too little context to judge)
            if chunks[i].word_count < 5 || chunks[j].word_count < 5 {
                continue;
            }

            let shared = shared_ngrams(&chunks[i].text, &chunks[j].text, 3);
            // Filter: N-gram must be uncommon AND contain at least one meaningful word (not all stop words)
            let meaningful_shared: Vec<&Vec<String>> = shared.iter()
                .filter(|ng| {
                    let freq = ngram_frequency.get(*ng).copied().unwrap_or(0);
                    if freq >= max_ngram_freq {
                        return false;
                    }
                    // At least one word must be a content word (not a stop word, ≥4 chars)
                    ng.iter().any(|w| w.len() >= 4 && !FRENCH_STOP_WORDS.contains(&w.as_str()))
                })
                .collect();

            // Require ≥2 meaningful shared N-grams to reduce false positives
            // (a single shared 3-word sequence is often coincidental in French)
            if meaningful_shared.len() < 2 {
                continue;
            }

            // Determine which is the retake (earlier) and which to keep (later)
            let remove_id = chunks[i].id;
            let keep_id = chunks[j].id;
            let ngram_text = meaningful_shared.iter()
                .take(2)
                .map(|ng| ng.join(" "))
                .collect::<Vec<_>>()
                .join(", ");

            // Skip if this pair is already in retake_pairs
            if retake_pairs.iter().any(|(r, k, _)| *r == remove_id && *k == keep_id) {
                continue;
            }

            retake_pairs.push((remove_id, keep_id, format!("N-gram partagé: \"{}\"", ngram_text)));

            if let (Some(a), Some(b)) = (chunks.get(i), chunks.get(j)) {
                let preview_a: String = a.text.chars().take(50).collect();
                let ellipsis_a = if a.text.chars().count() > 50 { "..." } else { "" };
                let preview_b: String = b.text.chars().take(50).collect();
                let ellipsis_b = if b.text.chars().count() > 50 { "..." } else { "" };

                let confidence = if is_truncated(&a.text) { "HAUTE" } else { "PROBABLE" };

                hints.push(format!(
                    "⚠️ REPRISE PAR CHEVAUCHEMENT (N-gram \"{}\"): [{}] puis [{}] → garder SEULEMENT [{}] (confiance: {})\n  [{}] \"{}{}\"\n  [{}] \"{}{}\"",
                    ngram_text, remove_id, keep_id, keep_id, confidence,
                    remove_id, preview_a, ellipsis_a,
                    keep_id, preview_b, ellipsis_b,
                ));
            }
        }
    }

    // === TIER 3: Content-word overlap (for truly different reformulations) ===
    for i in 0..chunks.len() {
        for j in (i + 1)..chunks.len() {
            let gap = chunks[j].start - chunks[i].end;
            if gap > 30.0 || gap < 0.0 {
                continue;
            }

            // Skip pairs already detected
            if retake_pairs.iter().any(|(r, k, _)| *r == chunks[i].id && *k == chunks[j].id) {
                continue;
            }

            if chunks[i].word_count < 5 || chunks[j].word_count < 5 {
                continue;
            }

            let content_a: HashSet<String> = extract_content_words(&chunks[i].text).into_iter().collect();
            let content_b: HashSet<String> = extract_content_words(&chunks[j].text).into_iter().collect();

            let shared_count = content_a.intersection(&content_b).count();
            let union_count = content_a.union(&content_b).count();

            // Lower threshold for short chunks (they have fewer content words so need lower absolute count)
            let min_shared = if chunks[i].word_count < 15 || chunks[j].word_count < 15 { 2 } else { 3 };
            if shared_count < min_shared || union_count == 0 {
                continue;
            }

            let jaccard = shared_count as f64 / union_count as f64;
            if jaccard < 0.20 {
                continue;
            }

            // Flag as retake if: truncated sentence, high overlap, or short chunk with decent overlap
            let is_aborted = is_truncated(&chunks[i].text);
            let is_short = chunks[i].word_count < 15;
            if !is_aborted && !is_short && jaccard < 0.40 {
                continue; // Not confident enough for long non-truncated chunks
            }

            let remove_id = chunks[i].id;
            let keep_id = chunks[j].id;
            let shared_words: Vec<String> = content_a.intersection(&content_b).cloned().collect();

            retake_pairs.push((remove_id, keep_id, format!("content overlap: {:?}", &shared_words[..shared_words.len().min(4)])));

            if let (Some(a), Some(b)) = (chunks.get(i), chunks.get(j)) {
                let preview_a: String = a.text.chars().take(50).collect();
                let ellipsis_a = if a.text.chars().count() > 50 { "..." } else { "" };
                let preview_b: String = b.text.chars().take(50).collect();
                let ellipsis_b = if b.text.chars().count() > 50 { "..." } else { "" };

                hints.push(format!(
                    "🔄 REPRISE PROBABLE (mots partagés: {}): [{}] → [{}] (Jaccard: {:.0}%)\n  [{}] \"{}{}\"\n  [{}] \"{}{}\"",
                    shared_words.iter().take(4).cloned().collect::<Vec<_>>().join(", "),
                    remove_id, keep_id, jaccard * 100.0,
                    remove_id, preview_a, ellipsis_a,
                    keep_id, preview_b, ellipsis_b,
                ));
            }
        }
    }

    // === MULTI-CHUNK BLOCK DETECTION ===
    // When chunk A is a retake of chunk C, chunks between A and C that are continuations
    // (SUITE markers or no overlap with C) should also be removed.
    let mut additional_removes: Vec<(usize, usize, String)> = Vec::new(); // (remove_id, keep_id, reason)

    for &(remove_id, keep_id, ref _reason) in &retake_pairs {
        if keep_id <= remove_id + 1 {
            continue; // No chunks between them
        }

        for between_id in (remove_id + 1)..keep_id {
            if let Some(between_chunk) = chunks.get(between_id) {
                // Already marked for removal? Skip.
                if retake_pairs.iter().any(|(r, _, _)| *r == between_id) {
                    continue;
                }
                if additional_removes.iter().any(|(r, _, _)| *r == between_id) {
                    continue;
                }

                // Is it a continuation (starts lowercase = SUITE)?
                let starts_lowercase = between_chunk.text.chars().next()
                    .map(|c| c.is_lowercase()).unwrap_or(false);

                // Is it very short (likely a fragment)?
                let is_fragment = between_chunk.word_count < 3;

                // Does it have NO content overlap with the keep chunk?
                let keep_chunk = chunks.get(keep_id);
                let no_overlap = keep_chunk.map(|kc| {
                    count_shared_content_words(&between_chunk.text, &kc.text) == 0
                }).unwrap_or(false);

                // Be conservative: only include if it's clearly a continuation of the failed attempt
                // (starts lowercase = SUITE) or a tiny fragment, AND has no overlap with keep chunk
                if is_fragment || (starts_lowercase && no_overlap) {
                    additional_removes.push((between_id, keep_id,
                        format!("partie du bloc de reprise [{}-{}]", remove_id, keep_id)));

                    let preview: String = between_chunk.text.chars().take(50).collect();
                    let ellipsis = if between_chunk.text.chars().count() > 50 { "..." } else { "" };
                    hints.push(format!(
                        "  ↳ [{}] fait partie de la tentative ratée [{}-{}] → supprimer\n    \"{}{}\"",
                        between_id, remove_id, keep_id, preview, ellipsis,
                    ));
                }
            }
        }
    }

    // Merge additional removes into retake_pairs
    retake_pairs.extend(additional_removes);

    if hints.is_empty() {
        return String::new();
    }

    let mut result = String::from(
        "=== REPRISES PRÉ-DÉTECTÉES ===\n\
        Les groupes suivants ont été identifiés comme des reprises potentielles.\n\
        - ⚠️ REPRISES DÉTECTÉES (même ouverture) → FIABLES, suis-les\n\
        - ⚠️ REPRISE PAR CHEVAUCHEMENT / 🔄 REPRISE PROBABLE → SUGGESTIONS, utilise ton jugement\n\n"
    );
    for hint in &hints {
        result.push_str(hint);
        result.push('\n');
    }
    result.push('\n');

    let opener_count = opener_groups.values().filter(|ids| ids.len() >= 2).count();
    eprintln!("Retake hints: {} opener groups, {} total hints ({} pairs detected)",
        opener_count, hints.len(), retake_pairs.len());
    result
}

/// Send the full transcript (as chunks) to Claude Sonnet in ONE call.
pub async fn determine_keep_ranges(
    chunks: &[SpeechChunk],
    api_key: &str,
    mode: &str,
) -> Result<Vec<usize>> {
    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    let retake_hints = build_retake_hints(chunks);

    let mut transcript = String::new();
    for (i, chunk) in chunks.iter().enumerate() {
        if i > 0 {
            let gap = chunk.start - chunks[i - 1].end;
            if gap >= 1.0 {
                transcript.push_str(&format!("  --- {:.1}s ---\n", gap));
            }
        }
        // Mark continuations: chunk starts with lowercase = continuation of previous sentence
        let continuation_marker = if chunk.text.chars().next().map(|c| c.is_lowercase()).unwrap_or(false) {
            " ⟵ SUITE"
        } else {
            ""
        };
        transcript.push_str(&format!(
            "[{}] {}-{} ({:.1}s, {} mots){} {}\n",
            chunk.id,
            format_time(chunk.start),
            format_time(chunk.end),
            chunk.end - chunk.start,
            chunk.word_count,
            continuation_marker,
            chunk.text,
        ));
    }

    let system_prompt = format!(
        r#"Tu es un assistant de montage vidéo expert. Tu analyses la transcription brute d'un rush vidéo pour déterminer les moments à GARDER dans le montage final.

La transcription est découpée en segments de parole numérotés. Chaque segment est un bloc continu de parole. Les silences entre segments sont automatiquement supprimés.

## TON TRAVAIL
Utilise ton raisonnement interne (thinking) pour analyser SYSTÉMATIQUEMENT la transcription, puis retourne la liste des IDs de segments à GARDER via l'outil report_keep_segments.

### MÉTHODE D'ANALYSE (dans ton thinking) :
1. Parcours la transcription de haut en bas
2. Pour chaque zone, identifie si le locuteur fait des REPRISES (même sujet répété)
3. Pour chaque groupe de reprises, identifie la DERNIÈRE VERSION COMPLÈTE
4. Vérifie que tu ne gardes qu'UNE SEULE VERSION par passage
5. VÉRIFICATION FINALE : relis ta liste et pour tout segment gardé, demande-toi "est-ce que ce contenu est déjà dit ailleurs dans un segment que je garde aussi ?" Si oui, supprime le doublon.

## RÈGLE N°1 — REPRISES (la plus importante !)
Le locuteur fait souvent PLUSIEURS TENTATIVES pour dire la même chose. Il peut y avoir 2, 5, 10, voire 20 tentatives d'un même passage !

COMMENT DÉTECTER UNE REPRISE :
- Plusieurs segments commencent par les mêmes mots ou abordent le même sujet
- Le locuteur s'arrête, puis recommence avec une formulation similaire ou différente
- Les tentatives sont proches dans le temps (quelques secondes à quelques minutes d'écart)
- ⚠️ IMPORTANT : les reprises ne sont PAS toujours mot-pour-mot identiques ! Le locuteur peut REFORMULER complètement entre deux tentatives.

### TYPES DE REPRISES À DÉTECTER :

1. REPRISE IDENTIQUE : mêmes mots au début
   [10] "Alors du coup on va utiliser... euh..."
   [11] "Alors du coup on va utiliser Cloud Code pour résoudre ce problème."
   → [10] = tentative ratée, garder SEULEMENT [11]

2. REPRISE REFORMULÉE : même sujet, mots différents
   [20] "Alors pour régler ce problème, on a eu— alors bien évidemment je crache pas sur..."
   [21] "Alors bien évidemment Cloud Code ça reste un outil incroyable surtout quand..."
   → [20] = tentative abandonnée, [21] = version finale. Garder SEULEMENT [21].

3. REPRISE MULTI-SEGMENTS : la tentative ratée s'étend sur plusieurs segments consécutifs
   [30] "Alors pour ce point-là je voulais dire que—"
   [31] "enfin c'est pas exactement ça mais on va dire que Cloud Code..."
   [32] "Alors Cloud Code ça reste un outil incroyable."
   → [30]+[31] = tentative ratée (bloc consécutif), [32] = version finale. Garder SEULEMENT [32].

4. ⚠️ REPRISES PROLONGÉES (PIÈGE FRÉQUENT) : après ce qui semble être la version finale, le locuteur fait ENCORE des tentatives !
   [40] "Et puis surtout ralfloop c'est rien de bien compliqué. Si on regarde ici..."  (tentative 1)
   [41] "Et puis surtout ralfloop c'est rien de bien sorcier. En fait le code..." (tentative 2)
   [42] "et puis surtout ralfloop c'est rien de bien compliqué. Au final c'est" (tentative 3)
   [43] "Et puis surtout ralfloop en fait il n'y a pas vraiment de valeur ajoutée. Si on regarde ici le code..." (tentative 4 — LONGUE, semble finale)
   --- mais ensuite ---
   [44] "Ou alors quand—" (fragment, ENCORE une tentative !)
   [45] "et puis surtout Ralph Loup en fait," (fragment)
   [46] "Et puis surtout ralfloop en fait c'est rien du tout" (tentative 5)
   [47] "et puis surtout ralfloop en fait il n'y a vraiment aucune valeur ajoutée, le cod..." (tentative 6 — version VRAIMENT finale)
   → Garder SEULEMENT [47]. Supprimer [40]-[46] y compris [43] qui semblait final mais ne l'est pas.
   ⚠️ La VRAIE dernière version est celle APRÈS laquelle le locuteur passe à un NOUVEAU sujet.

5. REPRISES DE PHRASES DE CONCLUSION : en fin de vidéo, le locuteur refait souvent sa conclusion/outro
   [100] "Et voilà, c'est tout bon, j'ai mon écran qui est prêt."
   [101] "j'aurais pu le faire dans une salle d'attente,"
   [102] "Et voilà, j'ai mon écran qui est prêt. Maintenant j'ai plus qu'à..." (version plus complète)
   [103] "j'aurais pu le faire au bord de la plage."
   → Garder la DERNIÈRE séquence complète. Si [102]+[103] est la dernière tentative, garder [102]+[103] et supprimer [100]+[101].

### SIGNAUX D'UNE TENTATIVE RATÉE :
- Se termine par "—", "...", ou mid-phrase (phrase inachevée)
- Contient des hésitations ("euh", "enfin", "c'est-à-dire")
- Est suivie d'une pause puis d'un redémarrage sur le même sujet
- Le segment suivant reprend la même idée de façon plus fluide/complète
- Est PLUS COURTE que la tentative suivante sur le même sujet

QUE FAIRE :
- Garder UNIQUEMENT la DERNIÈRE tentative complète
- Supprimer TOUTES les tentatives précédentes ET les tentatives intermédiaires
- La DERNIÈRE tentative = celle après laquelle le locuteur change VRAIMENT de sujet
- ⚠️ VÉRIFICATION : si tu gardes un segment et que 2-3 segments plus loin il y a un segment qui dit la même chose, c'est que tu as gardé une tentative intermédiaire. Supprime-la !

ANTI-FRANKENSTEIN :
- Chaque tentative = un BLOC de segments consécutifs
- Garder UN SEUL BLOC entier, supprimer les autres ENTIÈREMENT
- INTERDIT de garder le début d'une tentative + la fin d'une autre

## RÈGLE N°2 — Faux départs et fragments
- Segments très courts (<7 mots) entre deux reprises → quasi-certainement des faux départs → supprimer
- Phrases commencées mais jamais finies → supprimer
- Segments qui se terminent par "—" → supprimer (tentative abandonnée)

## RÈGLE N°2.5 — Segments qui se continuent (⟵ SUITE)
Certains segments forment une PHRASE UNIQUE coupée par le découpage automatique.
Signal : le segment est marqué "⟵ SUITE" et commence par un mot en MINUSCULE.
→ Ces segments forment un BLOC INDIVISIBLE avec le segment précédent.
→ Tu ne peux JAMAIS supprimer [N] et garder [N+1] si [N+1] est marqué SUITE.
→ Garder ou supprimer le BLOC ENTIER (les deux ensemble).

ERREUR COURANTE : confondre le DÉBUT d'une phrase (chunk N) avec un faux départ parce qu'il ressemble à un chunk précédent. Si chunk N+1 est marqué SUITE, alors chunk N n'est PAS un faux départ, c'est le DÉBUT d'une phrase qui continue dans N+1.

## RÈGLE N°3 — Ce qu'il faut GARDER
- Tout contenu UNIQUE (dit une seule fois) → GARDER
- La DERNIÈRE version complète de chaque passage repris → GARDER
- Les segments LONGS (>10 mots) qui apportent du contenu narratif → GARDER sauf si c'est clairement une reprise
- En cas de doute sur un segment UNIQUE (pas dans un groupe de reprises) → GARDER

⚠️ MAIS pour les segments dans un groupe de reprises pré-détecté : ne garde que la DERNIÈRE VERSION, même si les autres sont longs. C'est la règle la plus importante.

## PIÈGE À ÉVITER : phrases similaires ≠ reprises !
Le locuteur utilise souvent les MÊMES EXPRESSIONS DE TRANSITION à différents moments de la vidéo :
- "Voilà ce dont je parlais..." peut apparaître à 5 endroits différents de la vidéo → PAS une reprise si les sujets sont différents
- "C'est tout bon", "Le problème c'est que", "Ici je vais" → expressions COURANTES en français
→ Une reprise = MÊME SUJET + MÊME CONTEXTE + proches dans le temps (<2 min d'écart typiquement)
→ Même expression + sujets différents + éloignés dans le temps = PAS une reprise, GARDER LES DEUX

## REPRISES PRÉ-DÉTECTÉES
Les reprises pré-détectées au début de la transcription ont été identifiées par analyse algorithmique.
- ⚠️ REPRISES DÉTECTÉES (même ouverture + contenu similaire) = FIABLE. Suis-les : garde SEULEMENT le dernier segment indiqué.
- ⚠️ REPRISE PAR CHEVAUCHEMENT = PROBABLE. Évalue avec ton jugement mais penche vers la suppression.
- 🔄 REPRISE PROBABLE = SUGGESTION. Vérifie si le contenu est vraiment similaire avant de supprimer.
- ↳ = segment intermédiaire partie d'une tentative ratée → supprimer avec le reste du bloc.

## AUTO-VÉRIFICATION (CRITIQUE)
Avant de retourner ta réponse, vérifie :
1. Pour chaque groupe de reprises pré-détecté : as-tu gardé UN SEUL segment (le dernier) ? Si tu en gardes 2+, c'est une erreur.
2. Y a-t-il des segments consécutifs ou proches que tu gardes ET qui disent la même chose ? Si oui, ne garde que le dernier.
3. Les segments très courts (<5 mots) que tu gardes — sont-ils des faux départs ? Si suivis par un segment similaire plus long, supprime-les.

{}"#,
        get_mode_instruction(mode)
    );

    let user_message = format!(
        "Voici la transcription du rush vidéo. Retourne les IDs des segments à GARDER.\n\n{}{}",
        retake_hints, transcript
    );

    let tool = serde_json::json!({
        "name": "report_keep_segments",
        "description": "Report which segments to keep in the final video",
        "input_schema": {
            "type": "object",
            "required": ["keep_ids"],
            "properties": {
                "keep_ids": {
                    "type": "array",
                    "items": {"type": "integer"},
                    "description": "List of segment IDs to keep in the final video, in chronological order"
                }
            }
        }
    });

    eprintln!("Calling Claude Sonnet with extended thinking for transcript analysis ({} chunks, mode: {})...", chunks.len(), mode);

    let result = call_anthropic_api(&system_prompt, &user_message, tool, "report_keep_segments", api_key, true).await?;

    let keep_ids: Vec<usize> = result.get("keep_ids")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let max_id = chunks.len();
    let valid_ids: Vec<usize> = keep_ids.into_iter()
        .filter(|&id| id < max_id)
        .collect();

    eprintln!("Claude: keep {}/{} chunks ({} removed)",
        valid_ids.len(), chunks.len(), chunks.len() - valid_ids.len());

    Ok(valid_ids)
}

fn format_time(seconds: f64) -> String {
    let mins = (seconds / 60.0) as u64;
    let secs = seconds % 60.0;
    format!("{}:{:05.2}", mins, secs)
}

fn get_mode_instruction(mode: &str) -> &'static str {
    match mode {
        "aggressive" => "Mode agressif : identifie toutes les reprises probables, y compris les cas ambigus.",
        "conservative" => "Mode conservateur : identifie UNIQUEMENT les reprises évidentes et indiscutables. Au moindre doute, garde le passage.",
        _ => "Mode modéré : identifie les reprises claires et probables. En cas de doute léger, garde le passage.",
    }
}

/// Call the Anthropic Messages API with tool_use for structured output.
/// When `use_thinking` is true, enables extended thinking with streaming
/// to keep the connection alive during the thinking phase.
async fn call_anthropic_api(
    system: &str,
    user_message: &str,
    tool: serde_json::Value,
    tool_name: &str,
    api_key: &str,
    use_thinking: bool,
) -> Result<serde_json::Value> {
    let request_body = if use_thinking {
        serde_json::json!({
            "model": "claude-sonnet-4-5-20250929",
            "max_tokens": 16000,
            "thinking": {
                "type": "enabled",
                "budget_tokens": 10000
            },
            "stream": true,
            "system": system,
            "tools": [tool],
            "tool_choice": {"type": "auto"},
            "messages": [{"role": "user", "content": user_message}]
        })
    } else {
        serde_json::json!({
            "model": "claude-sonnet-4-5-20250929",
            "max_tokens": 8192,
            "system": system,
            "tools": [tool],
            "tool_choice": {"type": "tool", "name": tool_name},
            "messages": [{"role": "user", "content": user_message}]
        })
    };

    // Log request size for debugging
    let request_str = serde_json::to_string(&request_body).unwrap_or_default();
    let approx_size = request_str.len();
    eprintln!("API request size: {} chars ({}KB), use_thinking: {}", approx_size, approx_size / 1024, use_thinking);

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(if use_thinking { 600 } else { 120 }))
        .build()
        .context("Failed to build HTTP client")?;

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .context("Failed to send request to Anthropic API")?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        anyhow::bail!("Anthropic API error ({}): {}", status, error_body);
    }

    if use_thinking {
        // Streaming mode: parse SSE events to keep connection alive during thinking
        parse_streaming_response(response, tool_name).await
    } else {
        // Non-streaming: parse JSON response directly
        let api_response: AnthropicResponse = response.json().await
            .context("Failed to parse Anthropic API response")?;

        for block in api_response.content {
            if let AnthropicContentBlock::ToolUse { name, input, .. } = block {
                if name == tool_name {
                    return Ok(input);
                }
            }
        }
        anyhow::bail!("No tool_use block found in response for tool '{}'", tool_name)
    }
}

/// Parse a streaming SSE response from the Anthropic API.
/// Accumulates thinking text and tool_use input JSON from delta events.
async fn parse_streaming_response(
    mut response: reqwest::Response,
    tool_name: &str,
) -> Result<serde_json::Value> {
    let mut thinking_text = String::new();
    let mut tool_json = String::new();
    let mut found_tool = false;
    let mut current_block_type = String::new();
    let mut events_count = 0u32;

    // Read the response body chunk by chunk to keep connection alive
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.context("Failed to read streaming chunk")? {
        body.extend_from_slice(&chunk);
    }
    let body_text = String::from_utf8_lossy(&body);

    // Parse SSE events
    for line in body_text.lines() {
        let line = line.trim();
        if !line.starts_with("data: ") {
            continue;
        }
        let data = &line[6..];
        if data == "[DONE]" {
            break;
        }

        let event: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");
        events_count += 1;

        match event_type {
            "content_block_start" => {
                if let Some(block) = event.get("content_block") {
                    current_block_type = block.get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
                    if current_block_type == "tool_use" {
                        let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        if name == tool_name {
                            found_tool = true;
                        }
                    }
                }
            }
            "content_block_delta" => {
                if let Some(delta) = event.get("delta") {
                    match delta.get("type").and_then(|t| t.as_str()) {
                        Some("thinking_delta") => {
                            if let Some(t) = delta.get("thinking").and_then(|t| t.as_str()) {
                                thinking_text.push_str(t);
                            }
                        }
                        Some("input_json_delta") => {
                            if found_tool {
                                if let Some(j) = delta.get("partial_json").and_then(|j| j.as_str()) {
                                    tool_json.push_str(j);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            "content_block_stop" => {
                if current_block_type == "thinking" && !thinking_text.is_empty() {
                    let preview: String = thinking_text.chars().take(500).collect();
                    eprintln!("Claude thinking ({} chars): {}...", thinking_text.len(), preview);
                }
                current_block_type.clear();
            }
            _ => {}
        }
    }

    eprintln!("Streaming: processed {} SSE events", events_count);

    if tool_json.is_empty() {
        anyhow::bail!("No tool_use input found in streaming response (processed {} events, thinking: {} chars)",
            events_count, thinking_text.len());
    }

    let input: serde_json::Value = serde_json::from_str(&tool_json)
        .context("Failed to parse tool input JSON from streaming response")?;

    Ok(input)
}

/// Detect retake groups and abandoned fragments using Claude Sonnet via the Anthropic API.
pub async fn detect_retakes(
    passages: &[Passage],
    api_key: &str,
    mode: &str,
) -> Result<RetakeDetectionResult> {
    if passages.is_empty() {
        return Ok(RetakeDetectionResult { retake_groups: Vec::new(), abandoned_passages: Vec::new() });
    }

    let passage_list: Vec<serde_json::Value> = passages.iter().enumerate()
        .map(|(idx, p)| {
            let gap_after = if idx + 1 < passages.len() {
                let next = &passages[idx + 1];
                next.start - p.end
            } else {
                0.0
            };
            let duration = p.end - p.start;
            serde_json::json!({
                "id": p.id,
                "time": format!("{}-{}", format_time(p.start), format_time(p.end)),
                "duration": format!("{:.1}s", duration),
                "gap_after": format!("{:.1}s", gap_after),
                "words": p.word_count,
                "text": p.text,
            })
        })
        .collect();

    let system_prompt = format!(
        r#"Tu es un assistant de montage vidéo expert. Tu analyses la transcription d'un rush vidéo pour identifier ce qui doit être coupé.

Tu dois identifier DEUX types de contenu à supprimer :

## 1. GROUPES DE REPRISES (retake_groups)
Le locuteur fait plusieurs tentatives pour dire la même chose :
- Il commence un passage, s'arrête, puis recommence (faux départ suivi d'une meilleure version)
- Il fait 2, 3, 5+ tentatives pour formuler la même idée
- Les reprises peuvent être longues (plusieurs minutes) et couvrir plusieurs passages consécutifs

### REPRISES AVEC FORMULATION DIFFÉRENTE
Les reprises ne sont PAS forcément identiques mot pour mot ! Le locuteur peut reformuler entre deux tentatives. Indices d'une reprise même si les mots changent :
- Même ouverture de phrase ou thème similaire
- La première version se termine de façon abrupte, incomplète, ou part dans une mauvaise direction
- La deuxième version reprend au même point avec une meilleure formulation ou un angle différent
- Un gap notable entre les deux tentatives (> 1s)
- Le passage suivant ne continue PAS logiquement le précédent — il RECOMMENCE

### RÈGLE CRITIQUE : Garder la dernière version ENTIÈRE
- DANS CHAQUE GROUPE : identifie la DERNIÈRE VERSION COMPLÈTE et garde-la EN ENTIER
- Si la dernière version s'étend sur PLUSIEURS passages consécutifs, garde-les TOUS
- NE JAMAIS mélanger des passages de différentes tentatives
- Chaque tentative de reprise peut couvrir 1 ou plusieurs passages consécutifs. Traite-les comme un bloc indivisible.

## 2. PASSAGES ABANDONNÉS (abandoned_passages)
Passages isolés clairement incomplets ou inutiles :
- Phrases inachevées
- Fragments très courts (< 10 mots) qui ne forment pas une pensée complète
- Hésitations longues qui ne mènent nulle part

## NE PAS SUPPRIMER
- Des passages qui abordent des sujets similaires mais avec du contenu DIFFÉRENT
- Des phrases de transition récurrentes
- Des passages qui se complètent
- En cas de doute, NE PAS supprimer.

{}"#,
        get_mode_instruction(mode)
    );

    let user_message = format!(
        "Voici la transcription segmentée en passages. Identifie les groupes de reprises ET les passages abandonnés.\n\n{}",
        serde_json::to_string_pretty(&passage_list).unwrap_or_default()
    );

    let tool = serde_json::json!({
        "name": "report_retake_groups",
        "description": "Report the identified retake groups and abandoned passages from the transcript analysis",
        "input_schema": {
            "type": "object",
            "required": ["retake_groups", "abandoned_passages"],
            "properties": {
                "retake_groups": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["group_id", "description", "passages", "keep", "remove", "confidence"],
                        "properties": {
                            "group_id": {"type": "integer"},
                            "description": {"type": "string"},
                            "passages": {"type": "array", "items": {"type": "integer"}},
                            "keep": {"type": "array", "items": {"type": "integer"}},
                            "remove": {"type": "array", "items": {"type": "integer"}},
                            "confidence": {"type": "string", "enum": ["high", "medium", "low"]}
                        }
                    }
                },
                "abandoned_passages": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["id", "reason", "confidence"],
                        "properties": {
                            "id": {"type": "integer"},
                            "reason": {"type": "string"},
                            "confidence": {"type": "string", "enum": ["high", "medium", "low"]}
                        }
                    }
                }
            }
        }
    });

    eprintln!("Calling Claude Sonnet for retake detection ({} passages, mode: {})...", passages.len(), mode);

    let result = call_anthropic_api(&system_prompt, &user_message, tool, "report_retake_groups", api_key, false).await?;

    let retake_groups_val = result.get("retake_groups").cloned().unwrap_or(serde_json::json!([]));
    let all_groups: Vec<RetakeGroup> = serde_json::from_value(retake_groups_val).unwrap_or_default();

    let abandoned_val = result.get("abandoned_passages").cloned().unwrap_or(serde_json::json!([]));
    let all_abandoned: Vec<AbandonedPassage> = serde_json::from_value(abandoned_val).unwrap_or_default();

    let max_id = passages.len();
    let validated_groups: Vec<RetakeGroup> = all_groups.into_iter()
        .filter(|g| {
            g.passages.iter().all(|&id| id < max_id)
                && g.keep.iter().all(|&id| id < max_id)
                && g.remove.iter().all(|&id| id < max_id)
        })
        .collect();

    let validated_abandoned: Vec<AbandonedPassage> = all_abandoned.into_iter()
        .filter(|a| a.id < max_id)
        .collect();

    eprintln!("Claude identified {} retake groups and {} abandoned passages",
        validated_groups.len(), validated_abandoned.len());

    Ok(RetakeDetectionResult { retake_groups: validated_groups, abandoned_passages: validated_abandoned })
}

/// Verify proposed retake removals using a second Claude call.
pub async fn verify_retakes(
    passages: &[Passage],
    retake_groups: &[RetakeGroup],
    api_key: &str,
    mode: &str,
) -> Result<(Vec<usize>, Vec<GroupVerification>)> {
    if retake_groups.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let groups_to_verify: Vec<&RetakeGroup> = retake_groups.iter()
        .filter(|g| match mode {
            "aggressive" => true,
            "conservative" => g.confidence == "high",
            _ => g.confidence == "high" || g.confidence == "medium",
        })
        .collect();

    if groups_to_verify.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let all_remove_ids: std::collections::HashSet<usize> = groups_to_verify.iter()
        .flat_map(|g| g.remove.iter().copied())
        .collect();

    let remaining_preview: String = passages.iter()
        .filter(|p| !all_remove_ids.contains(&p.id))
        .map(|p| format!("[{}] {}", p.id, p.text))
        .collect::<Vec<_>>()
        .join("\n");

    let groups_summary: Vec<serde_json::Value> = groups_to_verify.iter()
        .map(|g| {
            let remove_texts: Vec<String> = g.remove.iter()
                .filter_map(|&id| passages.get(id))
                .map(|p| format!("[{}] \"{}\"", p.id, { let t: String = p.text.chars().take(100).collect(); if t.len() < p.text.len() { format!("{}...", t) } else { p.text.clone() } }))
                .collect();
            let keep_texts: Vec<String> = g.keep.iter()
                .filter_map(|&id| passages.get(id))
                .map(|p| format!("[{}] \"{}\"", p.id, { let t: String = p.text.chars().take(100).collect(); if t.len() < p.text.len() { format!("{}...", t) } else { p.text.clone() } }))
                .collect();
            serde_json::json!({
                "group_id": g.group_id,
                "description": g.description,
                "confidence": g.confidence,
                "remove": remove_texts,
                "keep": keep_texts,
            })
        })
        .collect();

    let system_prompt = format!(
        r#"Tu es un vérificateur de montage vidéo. On te donne une transcription originale et une liste de coupures proposées. Tu dois vérifier que chaque coupure est correcte.

Pour chaque groupe de reprises proposé, vérifie :
1. Les passages marqués "à supprimer" sont-ils vraiment des versions antérieures/inférieures du passage gardé ?
2. Le passage gardé contient-il bien l'essentiel du contenu des passages supprimés ?
3. Aucun contenu unique important n'est perdu par la suppression ?
4. Le flux narratif reste cohérent après suppression ?
5. ANTI-FRANKENSTEIN : Vérifie que le résultat ne mélange PAS des morceaux de différentes tentatives.

IMPORTANT : Sois CONSERVATEUR. En cas de doute, REJETTE la coupure (approved: false).

{}"#,
        get_mode_instruction(mode)
    );

    let user_message = format!(
        "COUPURES PROPOSÉES :\n{}\n\nAPERÇU DU RÉSULTAT (passages restants) :\n{}\n\nPour chaque groupe, indique s'il est approuvé ou rejeté.",
        serde_json::to_string_pretty(&groups_summary).unwrap_or_default(),
        remaining_preview
    );

    let tool = serde_json::json!({
        "name": "report_verification",
        "description": "Report verification results for proposed retake removals",
        "input_schema": {
            "type": "object",
            "required": ["verified_groups"],
            "properties": {
                "verified_groups": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["group_id", "approved", "reason"],
                        "properties": {
                            "group_id": {"type": "integer"},
                            "approved": {"type": "boolean"},
                            "reason": {"type": "string"}
                        }
                    }
                }
            }
        }
    });

    eprintln!("Calling Claude Sonnet for verification ({} groups to verify)...", groups_to_verify.len());

    let result = call_anthropic_api(&system_prompt, &user_message, tool, "report_verification", api_key, false).await?;

    let verification: VerificationResult = serde_json::from_value(result)
        .context("Failed to parse verification result")?;

    let approved_group_ids: std::collections::HashSet<usize> = verification.verified_groups.iter()
        .filter(|v| v.approved)
        .map(|v| v.group_id)
        .collect();

    let passages_to_remove: Vec<usize> = groups_to_verify.iter()
        .filter(|g| approved_group_ids.contains(&g.group_id))
        .flat_map(|g| g.remove.iter().copied())
        .collect();

    let approved_count = verification.verified_groups.iter().filter(|v| v.approved).count();
    let rejected_count = verification.verified_groups.iter().filter(|v| !v.approved).count();
    eprintln!("Verification: {}/{} groups approved, {} rejected",
        approved_count, groups_to_verify.len(), rejected_count);

    Ok((passages_to_remove, verification.verified_groups))
}
