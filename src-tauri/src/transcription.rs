use serde::{Deserialize, Serialize};
use anyhow::{Result, Context};
use reqwest::multipart;

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
        let chunk_result = transcribe_single_file(chunk_path_str, api_key, start).await
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

    let client = reqwest::Client::new();
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

pub fn detect_repetitions(phrases: &[Phrase], similarity_threshold: f64) -> Vec<usize> {
    let mut to_remove = Vec::new();

    // Only compare nearby phrases (within a window of 3)
    // Real repetitions happen when someone stutters/restarts - they're consecutive
    let window = 20;

    for i in 0..phrases.len() {
        if to_remove.contains(&i) {
            continue;
        }
        let end = (i + 1 + window).min(phrases.len());
        for j in (i + 1)..end {
            if to_remove.contains(&j) {
                continue;
            }

            // Both phrases need at least 3 words to be considered
            let words_i: Vec<&str> = phrases[i].text.split_whitespace().collect();
            let words_j: Vec<&str> = phrases[j].text.split_whitespace().collect();
            if words_i.len() < 3 || words_j.len() < 3 {
                continue;
            }

            let similarity = calculate_sequence_similarity(&words_i, &words_j);

            if similarity >= similarity_threshold {
                // Remove the FIRST occurrence (the false start),
                // keep the LAST one (the corrected version)
                to_remove.push(i);
                break;
            }
        }
    }

    to_remove.sort_unstable();
    to_remove.dedup();
    to_remove
}

/// Sequence-based similarity using longest common subsequence ratio.
/// Unlike bag-of-words, this respects word order.
fn calculate_sequence_similarity(words1: &[&str], words2: &[&str]) -> f64 {
    if words1.is_empty() || words2.is_empty() {
        return 0.0;
    }

    // Normalize words: lowercase, strip punctuation
    let norm1: Vec<String> = words1.iter().map(|w| normalize_word(w)).filter(|w| !w.is_empty()).collect();
    let norm2: Vec<String> = words2.iter().map(|w| normalize_word(w)).filter(|w| !w.is_empty()).collect();

    if norm1.is_empty() || norm2.is_empty() {
        return 0.0;
    }

    // Longest common subsequence (LCS)
    let n = norm1.len();
    let m = norm2.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];

    for i in 1..=n {
        for j in 1..=m {
            if norm1[i - 1] == norm2[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    let lcs_len = dp[n][m];
    // Ratio of LCS to the shorter phrase length
    let min_len = n.min(m);
    lcs_len as f64 / min_len as f64
}

fn normalize_word(word: &str) -> String {
    word.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}
