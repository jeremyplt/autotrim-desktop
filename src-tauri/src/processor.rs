use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;
use uuid::Uuid;
use anyhow::{Result, Context};

use crate::ffmpeg::{self, Segment};
use crate::transcription::{self, Phrase};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingSettings {
    pub mode: String,
    pub remove_silences: bool,
    pub remove_repetitions: bool,
    pub silence_threshold_db: f64,
    pub min_silence_duration: f64,
    pub repetition_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub stage: String,
    pub progress: f64,
    pub eta_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingResult {
    pub output_path: String,
    pub original_duration: f64,
    pub final_duration: f64,
    pub silences_removed: u32,
    pub repetitions_removed: u32,
}

#[derive(Debug)]
struct ProcessingJob {
    id: String,
    progress: Progress,
    result: Option<ProcessingResult>,
    canceled: bool,
}

static JOBS: Lazy<Arc<Mutex<HashMap<String, ProcessingJob>>>> = 
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

pub fn start_processing(
    video_path: String,
    settings: ProcessingSettings,
    api_key: String,
) -> String {
    let job_id = Uuid::new_v4().to_string();
    
    let job = ProcessingJob {
        id: job_id.clone(),
        progress: Progress {
            stage: "extracting".to_string(),
            progress: 0.0,
            eta_seconds: None,
        },
        result: None,
        canceled: false,
    };
    
    JOBS.lock().unwrap().insert(job_id.clone(), job);
    
    let job_id_clone = job_id.clone();
    tokio::spawn(async move {
        if let Err(e) = process_video(&job_id_clone, &video_path, &settings, &api_key).await {
            eprintln!("Processing error: {:?}", e);
            // Update job with error state
            if let Some(job) = JOBS.lock().unwrap().get_mut(&job_id_clone) {
                job.progress.stage = "error".to_string();
            }
        }
    });
    
    job_id
}

async fn process_video(
    job_id: &str,
    video_path: &str,
    settings: &ProcessingSettings,
    api_key: &str,
) -> Result<()> {
    let start_time = std::time::Instant::now();
    
    // Get video info
    let video_info = ffmpeg::get_video_info(video_path)
        .context("Failed to get video info")?;
    
    let total_duration = video_info.duration;
    
    // Create temp directory for processing
    let temp_dir = std::env::temp_dir().join(format!("autotrim_{}", job_id));
    std::fs::create_dir_all(&temp_dir)?;
    
    let audio_path = temp_dir.join("audio.wav");
    let audio_path_str = audio_path.to_str().unwrap();
    
    // Stage 1: Extract audio (0-20%)
    update_progress(job_id, "extracting", 5.0, None);
    ffmpeg::extract_audio(video_path, audio_path_str)?;
    update_progress(job_id, "extracting", 20.0, None);
    
    let mut segments_to_remove: Vec<Segment> = Vec::new();
    
    // Stage 2: Transcribe (20-40%)
    let mut phrases: Vec<Phrase> = Vec::new();
    if settings.remove_repetitions {
        update_progress(job_id, "transcribing", 25.0, None);
        let transcription = transcription::transcribe_audio(audio_path_str, api_key)
            .await
            .context("Failed to transcribe audio")?;
        update_progress(job_id, "transcribing", 40.0, None);
        
        phrases = transcription::segment_into_phrases(&transcription);
    } else {
        update_progress(job_id, "transcribing", 40.0, None);
    }
    
    // Stage 3: Detect silences (40-60%)
    if settings.remove_silences {
        update_progress(job_id, "detecting_silences", 45.0, None);
        let silences = ffmpeg::detect_silences(
            audio_path_str,
            settings.silence_threshold_db,
            settings.min_silence_duration,
        )?;
        segments_to_remove.extend(silences);
        update_progress(job_id, "detecting_silences", 60.0, None);
    } else {
        update_progress(job_id, "detecting_silences", 60.0, None);
    }
    
    // Stage 4: Detect repetitions (60-70%)
    let mut repetitions_removed = 0u32;
    if settings.remove_repetitions && !phrases.is_empty() {
        update_progress(job_id, "detecting_repetitions", 65.0, None);
        let repetition_indices = transcription::detect_repetitions(
            &phrases,
            settings.repetition_threshold,
        );
        
        repetitions_removed = repetition_indices.len() as u32;
        
        for idx in repetition_indices {
            if let Some(phrase) = phrases.get(idx) {
                segments_to_remove.push(Segment {
                    start: phrase.start,
                    end: phrase.end,
                });
            }
        }
        update_progress(job_id, "detecting_repetitions", 70.0, None);
    } else {
        update_progress(job_id, "detecting_repetitions", 70.0, None);
    }
    
    // Merge overlapping segments and create keep segments
    let segments_to_keep = calculate_keep_segments(&segments_to_remove, total_duration);
    
    // Calculate final duration
    let final_duration: f64 = segments_to_keep.iter()
        .map(|s| s.end - s.start)
        .sum();
    
    let silences_removed = (segments_to_remove.len() as u32).saturating_sub(repetitions_removed);
    
    // Stage 5: Render video (70-100%)
    update_progress(job_id, "rendering", 75.0, Some(30));
    
    let output_path = generate_output_path(video_path);
    ffmpeg::render_video(
        video_path,
        &segments_to_keep,
        &output_path,
        total_duration,
    )?;
    
    update_progress(job_id, "rendering", 100.0, Some(0));
    
    // Cleanup temp files
    let _ = std::fs::remove_dir_all(&temp_dir);
    
    // Store result
    let result = ProcessingResult {
        output_path,
        original_duration: total_duration,
        final_duration,
        silences_removed,
        repetitions_removed,
    };
    
    if let Some(job) = JOBS.lock().unwrap().get_mut(job_id) {
        job.result = Some(result);
        job.progress.progress = 100.0;
    }
    
    Ok(())
}

fn calculate_keep_segments(to_remove: &[Segment], total_duration: f64) -> Vec<Segment> {
    if to_remove.is_empty() {
        return vec![Segment { start: 0.0, end: total_duration }];
    }
    
    // Sort segments by start time
    let mut sorted = to_remove.to_vec();
    sorted.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
    
    // Merge overlapping segments
    let mut merged: Vec<Segment> = Vec::new();
    let mut current = sorted[0].clone();
    
    for segment in sorted.iter().skip(1) {
        if segment.start <= current.end {
            current.end = current.end.max(segment.end);
        } else {
            merged.push(current);
            current = segment.clone();
        }
    }
    merged.push(current);
    
    // Create keep segments (inverse of remove segments)
    let mut keep_segments = Vec::new();
    let mut last_end = 0.0;
    
    for segment in merged {
        if segment.start > last_end {
            keep_segments.push(Segment {
                start: last_end,
                end: segment.start,
            });
        }
        last_end = segment.end;
    }
    
    // Add final segment if needed
    if last_end < total_duration {
        keep_segments.push(Segment {
            start: last_end,
            end: total_duration,
        });
    }
    
    keep_segments
}

fn generate_output_path(input_path: &str) -> String {
    let path = Path::new(input_path);
    let parent = path.parent().unwrap_or(Path::new(""));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("mp4");
    
    let output_name = format!("{}_trimmed.{}", stem, ext);
    parent.join(output_name).to_str().unwrap().to_string()
}

fn update_progress(job_id: &str, stage: &str, progress: f64, eta_seconds: Option<u64>) {
    if let Some(job) = JOBS.lock().unwrap().get_mut(job_id) {
        if job.canceled {
            return;
        }
        job.progress = Progress {
            stage: stage.to_string(),
            progress,
            eta_seconds,
        };
    }
}

pub fn get_progress(job_id: &str) -> Option<Progress> {
    JOBS.lock().unwrap()
        .get(job_id)
        .map(|job| job.progress.clone())
}

pub fn get_result(job_id: &str) -> Option<ProcessingResult> {
    JOBS.lock().unwrap()
        .get(job_id)
        .and_then(|job| job.result.clone())
}

pub fn cancel_processing(job_id: &str) {
    if let Some(job) = JOBS.lock().unwrap().get_mut(job_id) {
        job.canceled = true;
    }
}
