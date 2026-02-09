use anyhow::{Result, Context};
use reqwest::multipart;
use serde::Deserialize;

use super::{Word, Transcription};

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
