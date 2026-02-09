mod whisper;
mod assemblyai;
pub mod analysis;

use serde::{Deserialize, Serialize};
use anyhow::Result;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TranscriptionProvider {
    Whisper,
    AssemblyAI,
}

impl TranscriptionProvider {
    pub fn from_str(s: &str) -> Self {
        match s {
            "assemblyai" => TranscriptionProvider::AssemblyAI,
            _ => TranscriptionProvider::Whisper,
        }
    }

    pub fn api_key_name(&self) -> &'static str {
        match self {
            TranscriptionProvider::Whisper => "OPENAI_API_KEY",
            TranscriptionProvider::AssemblyAI => "ASSEMBLYAI_API_KEY",
        }
    }
}

pub async fn transcribe_audio(
    audio_path: &str,
    api_key: &str,
    provider: &TranscriptionProvider,
) -> Result<Transcription> {
    match provider {
        TranscriptionProvider::Whisper => whisper::transcribe_audio(audio_path, api_key).await,
        TranscriptionProvider::AssemblyAI => assemblyai::transcribe_audio(audio_path, api_key).await,
    }
}

// Re-export analysis types and functions for backward compatibility
pub use analysis::{
    SpeechChunk,
    segment_into_chunks,
    determine_keep_ranges,
    // Legacy types still used by some code paths
    Phrase,
    Passage,
    RetakeGroup,
    AbandonedPassage,
    RetakeDetectionResult,
    GroupVerification,
    VerificationResult,
    segment_into_phrases,
    filter_filler_words,
    filter_filler_words_contextual,
    is_filler_word,
    segment_into_passages,
    detect_false_starts,
    detect_retake_sequences,
    detect_retakes,
    verify_retakes,
};
