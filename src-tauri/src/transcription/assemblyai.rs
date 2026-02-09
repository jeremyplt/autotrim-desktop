use anyhow::{Result, Context};
use serde::Deserialize;

use super::{Word, Transcription};

#[derive(Debug, Deserialize)]
struct UploadResponse {
    upload_url: String,
}

#[derive(Debug, Deserialize)]
struct TranscriptResponse {
    id: String,
    status: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    words: Option<Vec<AssemblyAIWord>>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    speech_model_used: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AssemblyAIWord {
    text: String,
    start: u64,  // milliseconds
    end: u64,    // milliseconds
    #[allow(dead_code)]
    confidence: f64,
}

/// Max poll time: 20 minutes (long videos can take a while)
const MAX_POLL_DURATION_SECS: u64 = 1200;
/// Poll interval: 5 seconds
const POLL_INTERVAL_SECS: u64 = 5;

pub async fn transcribe_audio(audio_path: &str, api_key: &str) -> Result<Transcription> {
    let file_size = std::fs::metadata(audio_path)
        .context("Failed to read audio file metadata")?
        .len();
    let file_size_mb = file_size as f64 / (1024.0 * 1024.0);
    eprintln!("AssemblyAI: uploading audio ({:.1}MB)...", file_size_mb);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600)) // 10 min upload timeout for large files
        .build()?;

    // Step 1: Upload the audio file
    let file_bytes = std::fs::read(audio_path)
        .context("Failed to read audio file")?;

    let upload_response = client
        .post("https://api.assemblyai.com/v2/upload")
        .header("authorization", api_key)
        .header("content-type", "application/octet-stream")
        .body(file_bytes)
        .send()
        .await
        .context("Failed to upload audio to AssemblyAI")?;

    let upload_status = upload_response.status();
    if !upload_status.is_success() {
        let error_body = upload_response.text().await.unwrap_or_default();
        anyhow::bail!("AssemblyAI upload error ({}): {}", upload_status, error_body);
    }

    let upload: UploadResponse = upload_response.json().await
        .context("Failed to parse AssemblyAI upload response")?;

    eprintln!("AssemblyAI: audio uploaded, requesting transcription...");

    // Step 2: Create transcription request
    // - speech_models: priority list, universal-3-pro first (best for FR), falls back to universal-2
    // - language_code: explicit French to avoid detection overhead
    // - disfluencies: true to capture filler words (euh, um) — our pipeline filters them downstream
    // - punctuate/format_text: true for better readability in Claude prompt
    let transcript_request = serde_json::json!({
        "audio_url": upload.upload_url,
        "speech_models": ["universal-3-pro", "universal-2"],
        "language_code": "fr",
        "punctuate": true,
        "format_text": true
    });

    let create_response = client
        .post("https://api.assemblyai.com/v2/transcript")
        .header("authorization", api_key)
        .header("content-type", "application/json")
        .json(&transcript_request)
        .send()
        .await
        .context("Failed to create AssemblyAI transcription")?;

    let create_status = create_response.status();
    if !create_status.is_success() {
        let error_body = create_response.text().await.unwrap_or_default();
        anyhow::bail!("AssemblyAI transcription request error ({}): {}", create_status, error_body);
    }

    let transcript: TranscriptResponse = create_response.json().await
        .context("Failed to parse AssemblyAI transcription response")?;

    let transcript_id = transcript.id;
    eprintln!("AssemblyAI: transcription queued (id: {}), polling...", transcript_id);

    // Step 3: Poll for completion
    let poll_start = std::time::Instant::now();
    let result = loop {
        if poll_start.elapsed().as_secs() > MAX_POLL_DURATION_SECS {
            anyhow::bail!("AssemblyAI transcription timed out after {}s", MAX_POLL_DURATION_SECS);
        }

        tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;

        let poll_response = client
            .get(format!("https://api.assemblyai.com/v2/transcript/{}", transcript_id))
            .header("authorization", api_key)
            .send()
            .await
            .context("Failed to poll AssemblyAI transcription")?;

        let poll_status = poll_response.status();
        if !poll_status.is_success() {
            let error_body = poll_response.text().await.unwrap_or_default();
            anyhow::bail!("AssemblyAI poll error ({}): {}", poll_status, error_body);
        }

        let result: TranscriptResponse = poll_response.json().await
            .context("Failed to parse AssemblyAI poll response")?;

        match result.status.as_str() {
            "completed" => {
                let model_used = result.speech_model_used.as_deref().unwrap_or("unknown");
                eprintln!("AssemblyAI: transcription completed in {:.0}s (model: {})",
                    poll_start.elapsed().as_secs_f64(), model_used);
                break result;
            }
            "error" => {
                let error_msg = result.error.unwrap_or_else(|| "Unknown error".to_string());
                anyhow::bail!("AssemblyAI transcription failed: {}", error_msg);
            }
            status => {
                eprintln!("  AssemblyAI status: {} ({:.0}s elapsed)", status, poll_start.elapsed().as_secs_f64());
            }
        }
    };

    // Step 4: Convert to our Transcription format
    // AssemblyAI timestamps are in milliseconds, convert to seconds
    let mut words: Vec<Word> = result.words
        .unwrap_or_default()
        .into_iter()
        .map(|w| Word {
            word: w.text.trim().to_string(),
            start: w.start as f64 / 1000.0,
            end: w.end as f64 / 1000.0,
        })
        .filter(|w| !w.word.is_empty())
        .collect();

    // AssemblyAI can return slightly out-of-order timestamps — sort to ensure monotonic
    words.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));

    let text = result.text.unwrap_or_default();

    eprintln!("AssemblyAI: {} words transcribed", words.len());

    Ok(Transcription { text, words })
}
