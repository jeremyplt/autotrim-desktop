use serde::{Deserialize, Serialize};
use anyhow::{Result, Context};
use reqwest::multipart;
use crate::ffmpeg::Segment;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub word: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcription {
    pub text: String,
    pub words: Vec<Word>,
}

// OpenAI Whisper API response structures
#[derive(Debug, Deserialize)]
struct WhisperApiResponse {
    text: String,
    #[serde(default)]
    words: Vec<WhisperApiWord>,
}

#[derive(Debug, Deserialize)]
struct WhisperApiWord {
    word: String,
    start: f64,
    end: f64,
}

/// Max chunk duration in seconds (~20 min at 64kbps ≈ 9.4MB, well under 25MB)
const CHUNK_DURATION_SECS: f64 = 1200.0;

pub async fn transcribe_audio(audio_path: &str, api_key: &str) -> Result<Transcription> {
    let file_size = std::fs::metadata(audio_path)
        .context("Failed to read audio file metadata")?
        .len();
    let file_size_mb = file_size as f64 / (1024.0 * 1024.0);
    eprintln!("Audio file size: {:.1}MB", file_size_mb);

    if file_size_mb <= 24.0 {
        // Small enough for a single API call
        return transcribe_single_file(audio_path, api_key, 0.0).await;
    }

    // Split into chunks and transcribe each
    eprintln!("File too large for single call, splitting into chunks...");
    let audio_dir = std::path::Path::new(audio_path).parent()
        .context("Invalid audio path")?;

    // Get total duration via ffprobe
    let duration_output = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1", audio_path])
        .output()
        .context("Failed to run ffprobe")?;
    let total_duration: f64 = String::from_utf8_lossy(&duration_output.stdout)
        .trim()
        .parse()
        .context("Failed to parse audio duration")?;

    let num_chunks = (total_duration / CHUNK_DURATION_SECS).ceil() as usize;
    eprintln!("Splitting {:.0}s audio into {} chunks of ~{:.0}s", total_duration, num_chunks, CHUNK_DURATION_SECS);

    let mut all_words: Vec<Word> = Vec::new();
    let mut all_text = String::new();

    for i in 0..num_chunks {
        let start = i as f64 * CHUNK_DURATION_SECS;
        let chunk_path = audio_dir.join(format!("chunk_{}.mp3", i));
        let chunk_path_str = chunk_path.to_str().unwrap();

        // Extract chunk with ffmpeg
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-ss", &format!("{}", start),
                "-i", audio_path,
                "-t", &format!("{}", CHUNK_DURATION_SECS),
                "-c", "copy",
                chunk_path_str,
            ])
            .status()
            .context("Failed to split audio chunk")?;

        if !status.success() {
            anyhow::bail!("FFmpeg failed to extract chunk {}", i);
        }

        eprintln!("Transcribing chunk {}/{} (offset: {:.0}s)...", i + 1, num_chunks, start);
        let chunk_result = transcribe_with_retry(chunk_path_str, api_key, start, 3).await
            .context(format!("Failed to transcribe chunk {}", i))?;

        all_text.push_str(&chunk_result.text);
        all_text.push(' ');
        all_words.extend(chunk_result.words);

        // Cleanup chunk file
        let _ = std::fs::remove_file(&chunk_path);
    }

    eprintln!("Transcription complete: {} total words", all_words.len());

    Ok(Transcription {
        text: all_text.trim().to_string(),
        words: all_words,
    })
}

/// Retry wrapper for transcription API calls.
async fn transcribe_with_retry(audio_path: &str, api_key: &str, time_offset: f64, max_retries: u32) -> Result<Transcription> {
    let mut last_error = None;
    for attempt in 0..max_retries {
        if attempt > 0 {
            let delay = std::time::Duration::from_secs(2u64.pow(attempt)); // 2s, 4s
            eprintln!("  Retry {}/{} after {}s...", attempt + 1, max_retries, delay.as_secs());
            tokio::time::sleep(delay).await;
        }
        match transcribe_single_file(audio_path, api_key, time_offset).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                eprintln!("  Transcription attempt {} failed: {}", attempt + 1, e);
                last_error = Some(e);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All transcription retries failed")))
}

async fn transcribe_single_file(audio_path: &str, api_key: &str, time_offset: f64) -> Result<Transcription> {
    let file_bytes = std::fs::read(audio_path)
        .context("Failed to read audio file")?;

    let file_name = std::path::Path::new(audio_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.mp3")
        .to_string();

    let file_part = multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str("audio/mpeg")?;

    let form = multipart::Form::new()
        .part("file", file_part)
        .text("model", "whisper-1")
        .text("response_format", "verbose_json")
        .text("timestamp_granularities[]", "word");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300)) // 5 min timeout for large chunks
        .build()?;
    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .context("Failed to send request to OpenAI Whisper API")?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI Whisper API error ({}): {}", status, error_body);
    }

    let api_response: WhisperApiResponse = response.json().await
        .context("Failed to parse OpenAI Whisper API response")?;

    let words: Vec<Word> = api_response.words.into_iter()
        .map(|w| Word {
            word: w.word.trim().to_string(),
            start: w.start + time_offset,
            end: w.end + time_offset,
        })
        .filter(|w| !w.word.is_empty())
        .collect();

    eprintln!("  -> {} words (offset {:.0}s)", words.len(), time_offset);

    Ok(Transcription {
        text: api_response.text,
        words,
    })
}

#[derive(Debug, Clone)]
pub struct Phrase {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub words: Vec<Word>,
}

pub fn segment_into_phrases(transcription: &Transcription) -> Vec<Phrase> {
    let mut phrases = Vec::new();
    let mut current_phrase_words: Vec<Word> = Vec::new();
    let mut current_text = String::new();

    let words = &transcription.words;
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
}

/// Segment words into passages for retake detection, skipping words inside removed segments.
/// Whisper word-level output has NO punctuation, so we split purely on pauses and word count.
/// - Any gap >= 0.7s → split (probable sentence/retake boundary)
/// - Passage >= 30 words AND gap >= 0.3s → split (prevent oversized passages)
/// - Passage >= 60 words → force-split at the best gap available
/// - Minimum 3 words per passage (merge with next if too short)
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
    // Track the best (largest) gap position within the current passage for force-splits
    let mut best_gap_in_passage: Option<(usize, f64)> = None; // (word index in current_words, gap size)

    for (idx, word) in meaningful_words.iter().enumerate() {
        if current_words.is_empty() {
            current_start = word.start;
            best_gap_in_passage = None;
        }

        current_words.push(word);
        current_text.push_str(&word.word);
        current_text.push(' ');

        let word_count = current_words.len();

        // Gap to next word
        let gap_after = if idx + 1 < meaningful_words.len() {
            meaningful_words[idx + 1].start - word.end
        } else {
            999.0 // force split at end
        };

        // Track the largest gap within this passage (for force-splitting large passages)
        if word_count >= 3 && gap_after < 999.0 {
            if let Some((_, best_gap)) = best_gap_in_passage {
                if gap_after > best_gap {
                    best_gap_in_passage = Some((word_count - 1, gap_after));
                }
            } else {
                best_gap_in_passage = Some((word_count - 1, gap_after));
            }
        }

        let should_split =
            // Any meaningful pause → split (retake/sentence boundary)
            gap_after >= 0.7
            // Passage getting big + small pause → split to keep passages manageable
            || (word_count >= 30 && gap_after >= 0.3)
            // Last word
            || idx == meaningful_words.len() - 1;

        // Force-split oversized passages at the best internal gap
        let force_split_large = word_count >= 60 && !should_split;

        if force_split_large {
            if let Some((split_at, _)) = best_gap_in_passage {
                // Split at the best gap position within the passage
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
                // Keep remaining words as start of next passage
                let remaining: Vec<&Word> = current_words[split_at + 1..].to_vec();
                current_words = remaining;
                current_text = current_words.iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" ");
                current_text.push(' ');
                current_start = current_words.first().map(|w| w.start).unwrap_or(word.end);
                best_gap_in_passage = None;
            }
            continue;
        }

        // Don't split if passage is too short (< 3 words) unless forced
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
async fn call_anthropic_api(
    system: &str,
    user_message: &str,
    tool: serde_json::Value,
    tool_name: &str,
    api_key: &str,
) -> Result<serde_json::Value> {
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-5-20250929",
        "max_tokens": 8192,
        "system": system,
        "tools": [tool],
        "tool_choice": {"type": "tool", "name": tool_name},
        "messages": [{"role": "user", "content": user_message}]
    });

    let client = reqwest::Client::new();
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

    let api_response: AnthropicResponse = response.json().await
        .context("Failed to parse Anthropic API response")?;

    // Find the tool_use block
    for block in api_response.content {
        if let AnthropicContentBlock::ToolUse { name, input, .. } = block {
            if name == tool_name {
                return Ok(input);
            }
        }
    }

    anyhow::bail!("No tool_use block found in Anthropic response for tool '{}'", tool_name)
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

    // Build enriched passage list for Claude with gap info
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
- DANS CHAQUE GROUPE : garde la meilleure/dernière version complète, supprime les autres

## 2. PASSAGES ABANDONNÉS (abandoned_passages)
Passages isolés clairement incomplets ou inutiles, sans version complète correspondante :
- Phrases inachevées : le locuteur commence une phrase mais ne la finit pas (ex: "Alors pour régler ce problème on a eu" → 8 mots, phrase tronquée, pas de suite)
- Fragments très courts (< 10 mots) qui ne forment pas une pensée complète
- Hésitations longues qui ne mènent nulle part
- INDICES : passage court + gap_after long = le locuteur a abandonné et est passé à autre chose

## INDICES DANS LES DONNÉES
- `gap_after` : pause après le passage. Un long gap (> 2s) après un passage court et incomplet = abandon probable.
- `duration` : durée du passage. Un passage de 2-3s avec peu de mots est souvent un faux départ.
- `words` : nombre de mots. Peu de mots + phrase incomplète = fragment à supprimer.
- Les reprises sont souvent consécutives ou proches dans le temps.

## NE PAS SUPPRIMER
- Des passages qui abordent des sujets similaires mais avec du contenu DIFFÉRENT
- Des phrases de transition récurrentes ("Donc", "Voilà", "Du coup") qui servent de liant
- Des passages qui se complètent (le locuteur AJOUTE une information, ne RÉPÈTE pas)
- Des rappels ou récapitulations intentionnels
- En cas de doute, NE PAS supprimer. Mieux vaut garder un passage en trop que supprimer du contenu unique.

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
                    "description": "Groups of passages where the speaker retries the same content. Each group has passages to keep and passages to remove.",
                    "items": {
                        "type": "object",
                        "required": ["group_id", "description", "passages", "keep", "remove", "confidence"],
                        "properties": {
                            "group_id": {"type": "integer"},
                            "description": {"type": "string", "description": "Brief description of what this retake group is about"},
                            "passages": {"type": "array", "items": {"type": "integer"}, "description": "All passage IDs in this retake group"},
                            "keep": {"type": "array", "items": {"type": "integer"}, "description": "Passage IDs to KEEP (best/final version)"},
                            "remove": {"type": "array", "items": {"type": "integer"}, "description": "Passage IDs to REMOVE (earlier/incomplete attempts)"},
                            "confidence": {"type": "string", "enum": ["high", "medium", "low"]}
                        }
                    }
                },
                "abandoned_passages": {
                    "type": "array",
                    "description": "Individual passage IDs that are clearly abandoned/incomplete fragments with no corresponding complete version. These are isolated false starts or unfinished sentences that should be removed.",
                    "items": {
                        "type": "object",
                        "required": ["id", "reason", "confidence"],
                        "properties": {
                            "id": {"type": "integer", "description": "Passage ID to remove"},
                            "reason": {"type": "string", "description": "Why this passage is abandoned/incomplete"},
                            "confidence": {"type": "string", "enum": ["high", "medium", "low"]}
                        }
                    }
                }
            }
        }
    });

    eprintln!("Calling Claude Sonnet for retake detection ({} passages, mode: {})...", passages.len(), mode);

    let result = call_anthropic_api(&system_prompt, &user_message, tool, "report_retake_groups", api_key).await?;

    // Parse retake_groups
    let retake_groups_val = result.get("retake_groups").cloned().unwrap_or(serde_json::json!([]));
    let all_groups: Vec<RetakeGroup> = serde_json::from_value(retake_groups_val).unwrap_or_default();

    // Parse abandoned_passages
    let abandoned_val = result.get("abandoned_passages").cloned().unwrap_or(serde_json::json!([]));
    let all_abandoned: Vec<AbandonedPassage> = serde_json::from_value(abandoned_val).unwrap_or_default();

    // Validate passage IDs
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
/// Returns the final list of passage IDs to remove (only approved groups).
pub async fn verify_retakes(
    passages: &[Passage],
    retake_groups: &[RetakeGroup],
    api_key: &str,
    mode: &str,
) -> Result<(Vec<usize>, Vec<GroupVerification>)> {
    if retake_groups.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // Filter by confidence based on mode
    let groups_to_verify: Vec<&RetakeGroup> = retake_groups.iter()
        .filter(|g| match mode {
            "aggressive" => true, // all confidence levels
            "conservative" => g.confidence == "high",
            _ => g.confidence == "high" || g.confidence == "medium", // moderate
        })
        .collect();

    if groups_to_verify.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // Build the remaining passages preview (what the final video would contain)
    let all_remove_ids: std::collections::HashSet<usize> = groups_to_verify.iter()
        .flat_map(|g| g.remove.iter().copied())
        .collect();

    let remaining_preview: String = passages.iter()
        .filter(|p| !all_remove_ids.contains(&p.id))
        .map(|p| format!("[{}] {}", p.id, p.text))
        .collect::<Vec<_>>()
        .join("\n");

    // Build groups summary for verification
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
        r#"Tu es un vérificateur de montage vidéo. On te donne une transcription originale et une liste de coupures proposées (reprises détectées). Tu dois vérifier que chaque coupure est correcte.

Pour chaque groupe de reprises proposé, vérifie :
1. Les passages marqués "à supprimer" sont-ils vraiment des versions antérieures/inférieures du passage gardé ?
2. Le passage gardé contient-il bien l'essentiel du contenu des passages supprimés ?
3. Aucun contenu unique important n'est perdu par la suppression ?
4. Le flux narratif reste cohérent après suppression ?

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
                            "reason": {"type": "string", "description": "Brief explanation of approval or rejection"}
                        }
                    }
                }
            }
        }
    });

    eprintln!("Calling Claude Sonnet for verification ({} groups to verify)...", groups_to_verify.len());

    let result = call_anthropic_api(&system_prompt, &user_message, tool, "report_verification", api_key).await?;

    let verification: VerificationResult = serde_json::from_value(result)
        .context("Failed to parse verification result")?;

    // Collect approved passage IDs to remove
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
