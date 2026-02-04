use serde::{Deserialize, Serialize};
use anyhow::{Result, Context};
use std::path::Path;

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

#[derive(Debug, Deserialize)]
struct WhisperResponse {
    text: String,
    words: Option<Vec<WhisperWord>>,
}

#[derive(Debug, Deserialize)]
struct WhisperWord {
    word: String,
    start: f64,
    end: f64,
}

pub async fn transcribe_audio(audio_path: &str, api_key: &str) -> Result<Transcription> {
    let client = reqwest::Client::new();
    
    // Read audio file
    let audio_data = tokio::fs::read(audio_path)
        .await
        .context("Failed to read audio file")?;
    
    let file_name = Path::new(audio_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.wav");
    
    // Create multipart form
    let part = reqwest::multipart::Part::bytes(audio_data)
        .file_name(file_name.to_string())
        .mime_str("audio/wav")?;
    
    let form = reqwest::multipart::Form::new()
        .text("model", "whisper-1")
        .text("response_format", "verbose_json")
        .text("timestamp_granularities[]", "word")
        .part("file", part);
    
    // Make API request
    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .context("Failed to send request to Whisper API")?;
    
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        anyhow::bail!("Whisper API error ({}): {}", status, error_text);
    }
    
    let whisper_response: WhisperResponse = response
        .json()
        .await
        .context("Failed to parse Whisper API response")?;
    
    let words = whisper_response.words
        .unwrap_or_default()
        .into_iter()
        .map(|w| Word {
            word: w.word,
            start: w.start,
            end: w.end,
        })
        .collect();
    
    Ok(Transcription {
        text: whisper_response.text,
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
    let mut current_phrase_words = Vec::new();
    let mut current_text = String::new();
    
    for word in &transcription.words {
        current_phrase_words.push(word.clone());
        current_text.push_str(&word.word);
        
        // End phrase on punctuation or long pause
        let ends_with_punctuation = word.word.trim_end().ends_with(|c: char| {
            c == '.' || c == '!' || c == '?' || c == ','
        });
        
        if ends_with_punctuation && !current_phrase_words.is_empty() {
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
    
    for i in 0..phrases.len() {
        for j in (i + 1)..phrases.len() {
            let similarity = calculate_similarity(&phrases[i].text, &phrases[j].text);
            
            if similarity >= similarity_threshold {
                // Keep the last occurrence (j), remove the earlier one (i)
                if !to_remove.contains(&i) {
                    to_remove.push(i);
                }
            }
        }
    }
    
    to_remove.sort_unstable();
    to_remove.dedup();
    to_remove
}

fn calculate_similarity(text1: &str, text2: &str) -> f64 {
    let words1: Vec<&str> = text1.split_whitespace().collect();
    let words2: Vec<&str> = text2.split_whitespace().collect();
    
    if words1.is_empty() || words2.is_empty() {
        return 0.0;
    }
    
    let mut matches = 0;
    let max_len = words1.len().max(words2.len());
    
    for word1 in &words1 {
        if words2.contains(word1) {
            matches += 1;
        }
    }
    
    matches as f64 / max_len as f64
}
