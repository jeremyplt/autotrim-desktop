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

/// Strip common French accents for normalization.
fn strip_accents(c: char) -> char {
    match c {
        'à' | 'â' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'î' | 'ï' => 'i',
        'ô' | 'ö' => 'o',
        'ù' | 'û' | 'ü' => 'u',
        'ÿ' => 'y',
        'ç' => 'c',
        'œ' => 'o', // simplified
        _ => c,
    }
}

/// Normalize a word for comparison: lowercase, strip accents, keep only alphanumeric chars.
fn normalize_word(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(strip_accents)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

// === IMPROVED RETAKE DETECTION (ported from Python) ===

/// Normalize text for similarity comparison: lowercase, strip accents, remove punctuation, normalize whitespace.
fn normalize_text_for_similarity(text: &str) -> String {
    let lower = text.to_lowercase();
    let no_punct: String = lower.chars()
        .map(strip_accents)
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    no_punct.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract n-grams (word windows) from text.
fn get_text_ngrams(text: &str, n: usize) -> Vec<Vec<String>> {
    let words: Vec<String> = text.split_whitespace().map(|s| s.to_string()).collect();
    if words.len() < n {
        return Vec::new();
    }
    words.windows(n).map(|w| w.to_vec()).collect()
}

/// Calculate n-gram Jaccard similarity between two texts.
/// Returns a value between 0.0 (no similarity) and 1.0 (identical).
fn ngram_similarity(text1: &str, text2: &str, n: usize) -> f64 {
    use std::collections::HashSet;
    
    let norm1 = normalize_text_for_similarity(text1);
    let norm2 = normalize_text_for_similarity(text2);
    
    let ngrams1: HashSet<Vec<String>> = get_text_ngrams(&norm1, n).into_iter().collect();
    let ngrams2: HashSet<Vec<String>> = get_text_ngrams(&norm2, n).into_iter().collect();
    
    if ngrams1.is_empty() || ngrams2.is_empty() {
        return 0.0;
    }
    
    let intersection = ngrams1.intersection(&ngrams2).count();
    let union = ngrams1.union(&ngrams2).count();
    
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Calculate sequence similarity using normalized Levenshtein distance.
/// Similar to Python's difflib.SequenceMatcher.ratio().
/// Returns a value between 0.0 (completely different) and 1.0 (identical).
fn sequence_matcher_similarity(text1: &str, text2: &str) -> f64 {
    let norm1 = normalize_text_for_similarity(text1);
    let norm2 = normalize_text_for_similarity(text2);
    strsim::normalized_damerau_levenshtein(&norm1, &norm2)
}

/// Detect groups of chunks that are retakes using content similarity.
/// Returns a list of retake groups where each group contains chunk IDs.
/// The LAST chunk in each group should be kept.
fn detect_retake_groups_advanced(
    chunks: &[SpeechChunk],
    time_window: f64,
    min_similarity: f64,
) -> Vec<Vec<usize>> {
    use std::collections::HashSet;
    
    let mut retake_groups: Vec<Vec<usize>> = Vec::new();
    let mut processed: HashSet<usize> = HashSet::new();
    
    for i in 0..chunks.len() {
        let chunk_i = &chunks[i];
        
        if processed.contains(&chunk_i.id) {
            continue;
        }
        
        // Look for similar chunks that come AFTER this one within the time window
        let mut group = vec![chunk_i.id];
        
        for j in (i + 1)..chunks.len() {
            let chunk_j = &chunks[j];
            
            if processed.contains(&chunk_j.id) {
                continue;
            }
            
            // Check if within time window from the FIRST chunk in the group
            if chunk_j.start - chunk_i.end > time_window {
                break;
            }
            
            // Compare candidate against ALL group members (transitive similarity)
            // This catches cases like: A ≈ B, B ≈ C, but A !≈ C
            let is_similar_to_any = group.iter().any(|&group_member_id| {
                if let Some(member) = chunks.get(group_member_id) {
                    let ngram_sim = ngram_similarity(&member.text, &chunk_j.text, 3);
                    let seq_sim = sequence_matcher_similarity(&member.text, &chunk_j.text);
                    let similarity = ngram_sim.max(seq_sim);
                    similarity >= min_similarity
                } else {
                    false
                }
            });
            
            if is_similar_to_any {
                group.push(chunk_j.id);
                processed.insert(chunk_j.id);
            }
        }
        
        if group.len() > 1 {
            retake_groups.push(group.clone());
            processed.insert(chunk_i.id);
        }
    }
    
    retake_groups
}

/// Build advanced retake hints using improved detection algorithm.
/// Generates explicit hints in the format expected by the improved prompt.
fn build_advanced_hints(chunks: &[SpeechChunk]) -> String {
    let time_window = 180.0; // 3 minutes
    let min_similarity = 0.35; // Optimal threshold from Python testing
    
    let retake_groups = detect_retake_groups_advanced(chunks, time_window, min_similarity);
    
    if retake_groups.is_empty() {
        return String::new();
    }
    
    let mut hints = Vec::new();
    hints.push("## REPRISES PRÉ-DÉTECTÉES (DÉTECTION AVANCÉE)\n".to_string());
    hints.push("Ces groupes ont été détectés algorithmiquement comme des REPRISES (même contenu répété).\n".to_string());
    hints.push("Pour chaque groupe, garde UNIQUEMENT le DERNIER chunk indiqué.\n\n".to_string());
    
    for (group_id, group) in retake_groups.iter().enumerate() {
        if group.is_empty() {
            continue;
        }
        
        // Get chunk texts for preview
        let mut chunk_texts = Vec::new();
        for &cid in group {
            if let Some(chunk) = chunks.get(cid) {
                let preview: String = chunk.text.chars().take(60).collect();
                let ellipsis = if chunk.text.len() > 60 { "..." } else { "" };
                chunk_texts.push(format!("  [{}] {}{}", cid, preview, ellipsis));
            }
        }
        
        let last_chunk_id = *group.last().unwrap();
        let remove_ids: Vec<usize> = group.iter().copied().filter(|&id| id != last_chunk_id).collect();
        
        hints.push(format!("⚠️ GROUPE DE REPRISES #{}:", group_id + 1));
        hints.push(format!("   Chunks: {:?}", group));
        hints.push(format!("   → GARDER SEULEMENT: [{}]", last_chunk_id));
        hints.push(format!("   → SUPPRIMER: {:?}", remove_ids));
        hints.push("".to_string());
        
        for text in chunk_texts {
            hints.push(text);
        }
        hints.push("".to_string());
    }
    
    eprintln!("Advanced retake detection: {} groups detected (similarity threshold: {})",
        retake_groups.len(), min_similarity);
    
    hints.join("\n") + "\n"
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

/// Extract the first 3 normalized words from a chunk's text as an "opener" tuple.
fn get_opener(text: &str) -> Option<(String, String, String)> {
    let words: Vec<String> = text.split_whitespace()
        .take(3)
        .map(|w| normalize_word(w))
        .collect();
    if words.len() == 3 {
        Some((words[0].clone(), words[1].clone(), words[2].clone()))
    } else {
        None
    }
}

/// Detect groups of chunks that are retakes via multiple strategies.
/// This is a FAITHFUL PORT of the Python retake_detector.py detect() function.
/// Returns (remove_id, keep_id, reason) pairs.
fn detect_all_retake_pairs(chunks: &[SpeechChunk]) -> Vec<(usize, usize, String)> {
    use std::collections::{HashMap, HashSet};

    let n = chunks.len();
    let mut rm: HashSet<usize> = HashSet::new(); // IDs to remove
    let mut pairs: Vec<(usize, usize)> = Vec::new(); // (remove_id, keep_id)

    // Opener frequency count
    let mut ofreq: HashMap<(String, String, String), usize> = HashMap::new();
    for c in chunks {
        if let Some(o) = get_opener(&c.text) {
            *ofreq.entry(o).or_insert(0) += 1;
        }
    }

    // ═══ S1: Opener groups with per-member keeper verification ═══
    let mut ogrp: HashMap<(String, String, String), Vec<usize>> = HashMap::new();
    for c in chunks {
        if let Some(o) = get_opener(&c.text) {
            if c.word_count >= 3 {
                ogrp.entry(o).or_default().push(c.id);
            }
        }
    }

    for (op, ids) in &ogrp {
        if ids.len() < 2 { continue; }
        let freq = ofreq.get(op).copied().unwrap_or(0);

        // Split by time (max 120s gap)
        let mut subs: Vec<Vec<usize>> = vec![vec![ids[0]]];
        for k in 1..ids.len() {
            let prev_idx = ids[k - 1];
            let curr_idx = ids[k];
            if let (Some(prev_chunk), Some(curr_chunk)) = (chunks.get(prev_idx), chunks.get(curr_idx)) {
                if curr_chunk.start - prev_chunk.end > 120.0 {
                    subs.push(vec![curr_idx]);
                } else {
                    subs.last_mut().unwrap().push(curr_idx);
                }
            }
        }

        for sub in &subs {
            if sub.len() < 2 { continue; }

            // Group-level overlap check: at least one pair shares content
            let min_grp_shared = if freq >= 4 { 3 } else { 2 };
            let mut has_grp_overlap = false;
            'outer: for a_idx in 0..sub.len() {
                for b_idx in (a_idx + 1)..sub.len() {
                    if let (Some(chunk_a), Some(chunk_b)) = (chunks.get(sub[a_idx]), chunks.get(sub[b_idx])) {
                        let shared = count_shared_content_words(&chunk_a.text, &chunk_b.text);
                        if shared >= min_grp_shared {
                            has_grp_overlap = true;
                            break 'outer;
                        }
                    }
                }
            }
            if !has_grp_overlap { continue; }

            let keep_id = *sub.last().unwrap();
            let keeper_chunk = match chunks.get(keep_id) {
                Some(c) => c,
                None => continue,
            };
            let cw_k: HashSet<String> = extract_content_words(&keeper_chunk.text).into_iter().collect();

            // Check if opener is "weak" (mostly stop/short words)
            let _weak_opener = op.0.len() <= 3 || op.1.len() <= 3 || op.2.len() <= 3;
            let stop_count = [&op.0, &op.1, &op.2].iter()
                .filter(|w| FRENCH_STOP_WORDS.contains(&w.as_str()) || w.len() <= 3)
                .count();
            let is_weak = stop_count >= 2;

            // Per-member verification against keeper
            for &cid in &sub[..sub.len() - 1] {
                let c = match chunks.get(cid) {
                    Some(ch) => ch,
                    None => continue,
                };
                let cw_c: HashSet<String> = extract_content_words(&c.text).into_iter().collect();
                let shared_set: HashSet<_> = cw_c.intersection(&cw_k).collect();
                let shared_count = shared_set.len();

                // Basic requirement: share at least 2 content words with keeper
                if shared_count < 2 { continue; }

                // For weak openers (common phrases), require stronger overlap
                if is_weak || freq >= 4 {
                    let cov_c = if !cw_c.is_empty() { shared_count as f64 / cw_c.len() as f64 } else { 0.0 };
                    let cov_k = if !cw_k.is_empty() { shared_count as f64 / cw_k.len() as f64 } else { 0.0 };
                    let (min_shared, min_cov) = if is_weak {
                        (4usize, 0.25) // Stricter for weak openers
                    } else if freq >= 6 {
                        (5, 0.20)
                    } else {
                        (4, 0.15)
                    };
                    if shared_count < min_shared || cov_c.min(cov_k) < min_cov {
                        continue;
                    }
                }

                rm.insert(cid);
                pairs.push((cid, keep_id));
            }
        }
    }

    // ═══ S2: Zone filling between retake pairs ═══
    for &(removed_id, keeper_id) in pairs.clone().iter() {
        for bid in (removed_id + 1)..keeper_id {
            if rm.contains(&bid) { continue; }
            let c = match chunks.get(bid) {
                Some(ch) => ch,
                None => continue,
            };
            let gap = if bid > 0 {
                if let Some(prev) = chunks.get(bid - 1) {
                    c.start - prev.end
                } else {
                    999.0
                }
            } else {
                999.0
            };
            let low = c.text.chars().next().map(|ch| ch.is_lowercase()).unwrap_or(false);
            let trunc = is_truncated(&c.text);
            let pr = if bid > 0 { rm.contains(&(bid - 1)) } else { false };
            let wc = c.word_count;

            let mut do_remove = false;
            if wc < 8 && pr && gap < 10.0 { do_remove = true; }
            if low && pr && gap < 5.0 && wc < 20 { do_remove = true; }
            if trunc && wc < 12 && pr { do_remove = true; }
            if wc < 5 && pr { do_remove = true; }

            // Content overlap with keeper
            if !do_remove && wc >= 5 {
                if let Some(keeper_chunk) = chunks.get(keeper_id) {
                    let ci: HashSet<String> = extract_content_words(&c.text).into_iter().collect();
                    let ck: HashSet<String> = extract_content_words(&keeper_chunk.text).into_iter().collect();
                    let sh: HashSet<_> = ci.intersection(&ck).collect();
                    let sh_count = sh.len();
                    if !ci.is_empty() && sh_count >= 3 && (sh_count as f64 / ci.len() as f64) >= 0.25 {
                        do_remove = true;
                    }
                }
            }

            if do_remove {
                rm.insert(bid);
                pairs.push((bid, keeper_id));
            }
        }
    }

    // ═══ S3: High-similarity content detection ═══
    // 200s window to catch sponsor-read retakes and long-range duplicates
    for i in 0..n {
        if rm.contains(&i) || chunks[i].word_count < 8 { continue; }
        let ci: HashSet<String> = extract_content_words(&chunks[i].text).into_iter().collect();
        if ci.len() < 3 { continue; }
        for j in (i + 1)..n {
            if rm.contains(&j) || chunks[j].word_count < 8 { continue; }
            let gap = chunks[j].start - chunks[i].end;
            if gap > 200.0 { break; }
            if gap < 0.0 { continue; }
            let cj: HashSet<String> = extract_content_words(&chunks[j].text).into_iter().collect();
            let sh: HashSet<_> = ci.intersection(&cj).collect();
            let un: HashSet<_> = ci.union(&cj).collect();
            let sh_count = sh.len();
            let un_count = un.len();

            // Tier 1: High Jaccard overlap
            if sh_count >= 5 && un_count > 0 && (sh_count as f64 / un_count as f64) >= 0.35 {
                rm.insert(i);
                pairs.push((i, j));
                break;
            }

            // Tier 2: High coverage of shorter chunk, later must be bigger
            if sh_count >= 5 {
                let min_len = ci.len().min(cj.len());
                let coverage = sh_count as f64 / min_len as f64;
                if coverage >= 0.55 && chunks[j].word_count > chunks[i].word_count {
                    rm.insert(i);
                    pairs.push((i, j));
                    break;
                }
            }
        }
    }

    // ═══ S4: Fragment/continuation cleanup ═══
    for _ in 0..5 {
        let mut changed = false;
        for i in 0..n {
            if rm.contains(&i) { continue; }
            let c = &chunks[i];
            let pr = i > 0 && rm.contains(&(i - 1));
            let nr = i < n - 1 && rm.contains(&(i + 1));
            let gb = if i > 0 {
                if let Some(prev) = chunks.get(i - 1) {
                    c.start - prev.end
                } else {
                    999.0
                }
            } else {
                999.0
            };
            let low = c.text.chars().next().map(|ch| ch.is_lowercase()).unwrap_or(false);
            let trunc = is_truncated(&c.text);
            let wc = c.word_count;

            let mut do_remove = false;
            if wc < 5 && pr && nr { do_remove = true; }
            if wc < 4 && pr && gb < 5.0 { do_remove = true; }
            if trunc && wc < 10 && (pr || nr) { do_remove = true; } // Increased from 8 to 10
            if low && wc < 15 && pr && gb < 5.0 { do_remove = true; } // Increased from 12 to 15

            // Truncated/abandoned sentence even without removed neighbors
            if !do_remove && trunc && wc < 12 {
                // Look if content is repeated in a later, longer chunk
                let ci: HashSet<String> = extract_content_words(&c.text).into_iter().collect();
                if !ci.is_empty() {
                    for j in (i + 1)..std::cmp::min(i + 8, n) {
                        if let Some(later) = chunks.get(j) {
                            if later.start - c.end > 60.0 { break; }
                            let cj: HashSet<String> = extract_content_words(&later.text).into_iter().collect();
                            let sh: HashSet<_> = ci.intersection(&cj).collect();
                            if !ci.is_empty() && (sh.len() as f64 / ci.len() as f64) >= 0.5 {
                                do_remove = true;
                                break;
                            }
                        }
                    }
                }
            }

            // Short chunk with most content in nearby later chunk
            // Expanded: search 10 chunks ahead, 120s window (was 5 chunks, 30s)
            if !do_remove && wc >= 3 && wc <= 15 {
                let ci: HashSet<String> = extract_content_words(&c.text).into_iter().collect();
                if !ci.is_empty() {
                    for j in (i + 1)..std::cmp::min(i + 10, n) {
                        if let Some(later_chunk) = chunks.get(j) {
                            if later_chunk.start - c.end > 120.0 { break; }
                            let cj: HashSet<String> = extract_content_words(&later_chunk.text).into_iter().collect();
                            let sh: HashSet<_> = ci.intersection(&cj).collect();
                            let sh_count = sh.len();
                            if sh_count >= 1 && (sh_count as f64 / ci.len() as f64) >= 0.5 && later_chunk.word_count > wc {
                                do_remove = true;
                                break;
                            }
                        }
                    }
                }
            }

            if do_remove {
                rm.insert(i);
                changed = true;
            }
        }
        if !changed { break; }
    }

    // ═══ S5: Non-French detection ═══
    let fr: HashSet<&str> = [
        "le", "la", "les", "un", "une", "des", "de", "du", "je", "tu", "il", "elle", "on",
        "nous", "vous", "et", "ou", "mais", "donc", "est", "sont", "pas", "plus", "dans", "sur",
        "avec", "pour", "que", "qui", "ça", "ce", "cette"
    ].iter().copied().collect();
    for c in chunks {
        if rm.contains(&c.id) { continue; }
        let wl: Vec<String> = c.text.split_whitespace()
            .map(|w| w.to_lowercase().trim_matches(|ch: char| ".,!?".contains(ch)).to_string())
            .collect();
        if !wl.is_empty() {
            let fr_count = wl.iter().filter(|w| fr.contains(w.as_str())).count();
            if (fr_count as f64 / wl.len() as f64) < 0.1 && c.word_count >= 3 {
                rm.insert(c.id);
            }
        }
    }

    // ═══ S6: Sandwiched and tiny chunk cleanup ═══
    // Catches fragments between removed chunks and very short orphans
    for _ in 0..3 {
        let mut changed = false;
        for i in 0..n {
            if rm.contains(&i) { continue; }
            let wc = chunks[i].word_count;
            let pr = i > 0 && rm.contains(&(i - 1));
            let nr = i < n - 1 && rm.contains(&(i + 1));
            let gb = if i > 0 { chunks[i].start - chunks[i - 1].end } else { 999.0 };
            let ga = if i < n - 1 { chunks[i + 1].start - chunks[i].end } else { 999.0 };

            let mut do_remove = false;
            // Sandwiched between removed, small gaps, short
            if pr && nr && gb < 10.0 && ga < 10.0 && wc <= 20 { do_remove = true; }
            // Tiny chunk adjacent to removed
            if wc <= 3 && (pr || nr) { do_remove = true; }
            // Short fragment adjacent to removed with small gap
            if wc <= 5 && ((pr && gb < 5.0) || (nr && ga < 5.0)) { do_remove = true; }

            if do_remove {
                rm.insert(i);
                changed = true;
            }
        }
        if !changed { break; }
    }
    eprintln!("S6 (sandwiched/tiny cleanup): {} total removed so far", rm.len());

    // ═══ S7: Superseded take detection ═══
    // When a later, longer chunk covers the same content as an earlier shorter one,
    // remove the earlier one. Works across larger time windows.
    // This catches: sponsor reads where person restarts after a gap.
    for i in 0..n {
        if rm.contains(&i) { continue; }
        let ci_words: HashSet<String> = extract_content_words(&chunks[i].text).into_iter().collect();
        if ci_words.len() < 3 { continue; }
        let wc_i = chunks[i].word_count;
        
        for j in (i + 1)..n {
            if rm.contains(&j) { continue; }
            let gap = chunks[j].start - chunks[i].end;
            if gap > 300.0 { break; } // 5 minute window
            if gap < 0.0 { continue; }
            
            let cj_words: HashSet<String> = extract_content_words(&chunks[j].text).into_iter().collect();
            if cj_words.len() < 3 { continue; }
            let wc_j = chunks[j].word_count;
            
            // Earlier chunk's content is mostly covered by later chunk
            let sh: HashSet<_> = ci_words.intersection(&cj_words).collect();
            let sh_count = sh.len();
            let coverage_i = sh_count as f64 / ci_words.len() as f64;
            
            // Require: later chunk covers ≥70% of earlier chunk's content
            // AND later chunk is significantly longer (≥2x words)
            if coverage_i >= 0.70 && wc_j as f64 >= wc_i as f64 * 2.0 && sh_count >= 4 {
                rm.insert(i);
                pairs.push((i, j));
                eprintln!("  S7: REMOVE [{}] ({} words) superseded by [{}] ({} words), coverage={:.0}%",
                    i, wc_i, j, wc_j, coverage_i * 100.0);
                break;
            }
        }
    }

    // ═══ S8: Zone-fill after S6/S7 ═══
    // Re-run zone filling for newly created retake pairs from S6/S7
    {
        let new_pairs: Vec<(usize, usize)> = pairs.iter()
            .filter(|(r, k)| rm.contains(r) && !rm.contains(k))
            .map(|(r, k)| (*r, *k))
            .collect();
        
        for &(removed_id, keeper_id) in &new_pairs {
            for bid in (removed_id + 1)..keeper_id {
                if rm.contains(&bid) { continue; }
                let c = match chunks.get(bid) {
                    Some(ch) => ch,
                    None => continue,
                };
                let gap = if bid > 0 {
                    if let Some(prev) = chunks.get(bid - 1) {
                        c.start - prev.end
                    } else { 999.0 }
                } else { 999.0 };
                let pr = bid > 0 && rm.contains(&(bid - 1));
                let wc = c.word_count;
                let low = c.text.chars().next().map(|ch| ch.is_lowercase()).unwrap_or(false);
                let trunc = is_truncated(&c.text);

                let mut do_remove = false;
                if wc < 8 && pr && gap < 10.0 { do_remove = true; }
                if low && pr && gap < 5.0 && wc < 20 { do_remove = true; }
                if trunc && wc < 12 && pr { do_remove = true; }
                if wc < 5 && pr { do_remove = true; }

                // Content overlap with keeper (stricter for longer chunks)
                if !do_remove && wc >= 5 {
                    if let Some(keeper_chunk) = chunks.get(keeper_id) {
                        let ci: HashSet<String> = extract_content_words(&c.text).into_iter().collect();
                        let ck: HashSet<String> = extract_content_words(&keeper_chunk.text).into_iter().collect();
                        let sh: HashSet<_> = ci.intersection(&ck).collect();
                        let sh_count = sh.len();
                        let min_cov = if wc > 20 { 0.40 } else { 0.25 };
                        let min_sh = if wc > 20 { 4 } else { 3 };
                        if !ci.is_empty() && sh_count >= min_sh && (sh_count as f64 / ci.len() as f64) >= min_cov {
                            do_remove = true;
                        }
                    }
                }

                if do_remove {
                    rm.insert(bid);
                    pairs.push((bid, keeper_id));
                }
            }
        }
    }

    // ═══ S9: Extended zone cleanup ═══
    for _ in 0..3 {
        let mut changed = false;
        for i in 0..n {
            if rm.contains(&i) { continue; }
            let wc = chunks[i].word_count;
            let gb = if i > 0 { chunks[i].start - chunks[i - 1].end } else { 999.0 };

            let mut rm_before = 0usize;
            let mut j = i as isize - 1;
            while j >= 0 && rm.contains(&(j as usize)) { rm_before += 1; j -= 1; }
            let mut rm_after = 0usize;
            let mut j = i + 1;
            while j < n && rm.contains(&j) { rm_after += 1; j += 1; }

            let mut do_remove = false;
            if rm_before >= 2 && rm_after >= 2 && wc <= 15 { do_remove = true; }
            if rm_before >= 3 && wc <= 10 && gb < 5.0 { do_remove = true; }

            if do_remove { rm.insert(i); changed = true; }
        }
        if !changed { break; }
    }

    // ═══ S10: Orphan fragment cleanup ═══
    for i in 0..n {
        if rm.contains(&i) { continue; }
        let wc = chunks[i].word_count;
        if wc > 8 { continue; }
        let pr = i > 0 && rm.contains(&(i - 1));
        let ga = if i < n - 1 { chunks[i + 1].start - chunks[i].end } else { 999.0 };
        if pr && ga > 20.0 && wc <= 8 {
            rm.insert(i);
        }
    }

    eprintln!("Total after all strategies (S1-S10): {} chunks to remove out of {}", rm.len(), n);

    // Convert keep set to remove pairs (Python returns KEEP ids, Rust returns REMOVE pairs)
    let keep_set: HashSet<usize> = (0..n).filter(|i| !rm.contains(i)).collect();
    let mut result_pairs: Vec<(usize, usize, String)> = Vec::new();

    // For each removed chunk, find the nearest kept chunk as the "keep_id"
    for &remove_id in &rm {
        // Check if it's already in pairs (from S1/S2)
        if let Some(&(_, keep_id)) = pairs.iter().find(|(r, _)| *r == remove_id) {
            result_pairs.push((remove_id, keep_id, "S1-S2".to_string()));
        } else {
            // Find nearest kept chunk (prefer later chunks)
            let mut best_keep = 0;
            for j in (remove_id + 1)..n {
                if keep_set.contains(&j) {
                    best_keep = j;
                    break;
                }
            }
            if best_keep == 0 {
                // No later kept chunk, find earlier
                for j in (0..remove_id).rev() {
                    if keep_set.contains(&j) {
                        best_keep = j;
                        break;
                    }
                }
            }
            if best_keep != 0 {
                result_pairs.push((remove_id, best_keep, "S3-S5".to_string()));
            }
        }
    }

    eprintln!("Algorithm: {} chunks to remove out of {} (S1-S8)", rm.len(), n);
    result_pairs
}
/// NEW STRATEGY: Pre-remove algorithmic retakes, send clean transcript to Claude.
pub async fn determine_keep_ranges(
    chunks: &[SpeechChunk],
    api_key: &str,
    mode: &str,
) -> Result<Vec<usize>> {
    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    // === PHASE 1: Algorithmic pre-removal ===
    let retake_pairs = detect_all_retake_pairs(chunks);
    let algo_remove: std::collections::HashSet<usize> = retake_pairs.iter().map(|(r, _, _)| *r).collect();

    eprintln!("Phase 1 (algorithmic): removing {}/{} chunks", algo_remove.len(), chunks.len());
    for (remove_id, keep_id, reason) in &retake_pairs {
        if let Some(chunk) = chunks.get(*remove_id) {
            let preview: String = chunk.text.chars().take(50).collect();
            eprintln!("  REMOVE [{}] → KEEP [{}] ({}): \"{}...\"", remove_id, keep_id, reason, preview);
        }
    }

    // Build transcript with ONLY surviving chunks (renumber for Claude)
    let surviving_chunks: Vec<&SpeechChunk> = chunks.iter()
        .filter(|c| !algo_remove.contains(&c.id))
        .collect();

    eprintln!("Phase 2 (Claude): analyzing {} surviving chunks", surviving_chunks.len());

    let mut transcript = String::new();
    for (i, chunk) in surviving_chunks.iter().enumerate() {
        if i > 0 {
            let prev = surviving_chunks[i - 1];
            let gap = chunk.start - prev.end;
            if gap >= 1.0 {
                transcript.push_str(&format!("  --- {:.1}s ---\n", gap));
            }
        }
        let continuation_marker = if chunk.text.chars().next().map(|c| c.is_lowercase()).unwrap_or(false) {
            " ⟵ SUITE"
        } else {
            ""
        };
        // Use ORIGINAL IDs so we can map back
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
        r#"Tu es un assistant de montage vidéo expert. Tu analyses une transcription PRÉ-NETTOYÉE d'un rush vidéo pour créer un montage final professionnel.

Les reprises évidentes ont DÉJÀ été supprimées automatiquement. Tu vois uniquement les segments survivants. Ton travail est de nettoyer DAVANTAGE.

## TON TRAVAIL
Identifie et SUPPRIME tous les segments qui ne devraient PAS apparaître dans le montage final. Retourne les IDs des segments à GARDER.

## CE QUE TU DOIS SUPPRIMER

### 1. Reprises subtiles
Deux segments qui disent la même chose avec des mots différents, même si éloignés dans le temps (jusqu'à 5 min). Garde UNIQUEMENT la meilleure version (généralement la dernière).

### 2. Faux départs et phrases abandonnées
- Segment très court (<8 mots) suivi d'un segment similaire plus complet
- Segment qui se termine abruptement ou de manière incomplète
- Segment qui recommence une idée déjà mieux exprimée ailleurs

### 3. Dictée de prompts/instructions à une IA
Sections où le locuteur dicte des instructions détaillées à un agent IA ou un système (ex: "fais en sorte que...", "mets un bouton...", "crée une nouvelle branche..."). Ces sections techniques de dictation doivent être SUPPRIMÉES car elles ne sont pas intéressantes pour le spectateur. SAUF si le locuteur explique le concept au spectateur.

### 4. Sections de débogage/attente
- Moments où le locuteur attend un résultat ("on va attendre...", "pas encore...")
- Sections de troubleshooting en temps réel
- Conversations de debugging avec une IA ("casse ça", "corrige ça", "envoie-moi le lien")

### 5. Contenu hors-sujet
Tout contenu qui n'est clairement pas lié au sujet principal de la vidéo (digressions, conversations en arrière-plan, contenu sans rapport)

### 6. Versions multiples de la conclusion
Si plusieurs tentatives de conclusion/outro existent, ne garde que la DERNIÈRE version complète.

## CE QUE TU DOIS GARDER
- Contenu explicatif unique destiné au spectateur
- Démonstrations visuelles commentées (le locuteur montre quelque chose à l'écran)
- Résultats et réactions aux résultats ("ça marche!", "voilà le résultat")
- L'introduction et la conclusion finale

## SEGMENTS ⟵ SUITE
= continuation du segment précédent. Garder ou supprimer ensemble, jamais séparément.

## RETOURNE
La liste des IDs des segments à GARDER (dans l'ordre chronologique).
En cas de doute sur du contenu technique/dictation → SUPPRIME.
En cas de doute sur du contenu explicatif unique → GARDE.

## {}"#,
        get_mode_instruction(mode)
    );

    let user_message = format!(
        "Transcription pré-nettoyée ({} segments, {} déjà supprimés par l'algorithme). Retourne les IDs à GARDER.\n\n{}",
        surviving_chunks.len(), algo_remove.len(), transcript
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

    eprintln!("Calling Claude Sonnet for remaining retake detection ({} chunks, mode: {})...", surviving_chunks.len(), mode);

    let result = call_anthropic_api(&system_prompt, &user_message, tool, "report_keep_segments", api_key, true).await?;

    let keep_ids: Vec<usize> = result.get("keep_ids")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Valid IDs must be from the original chunks AND not already removed
    let surviving_ids: std::collections::HashSet<usize> = surviving_chunks.iter().map(|c| c.id).collect();
    let valid_ids: Vec<usize> = keep_ids.into_iter()
        .filter(|id| surviving_ids.contains(id))
        .collect();

    let claude_removed = surviving_chunks.len() - valid_ids.len();
    eprintln!("Phase 2 (Claude): kept {}/{} surviving chunks ({} additional removals)",
        valid_ids.len(), surviving_chunks.len(), claude_removed);
    eprintln!("Total: {}/{} chunks kept ({} algo + {} Claude removed)",
        valid_ids.len(), chunks.len(), algo_remove.len(), claude_removed);

    Ok(valid_ids)
}

// Old hint-based approach removed — now using direct algorithmic removal + Claude verification

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
