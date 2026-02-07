use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::path::Path;
use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;
use uuid::Uuid;
use anyhow::{Result, Context};

use crate::ffmpeg::{self, Segment};
use crate::transcription;

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
    tauri::async_runtime::spawn(async move {
        if let Err(e) = process_video(&job_id_clone, &video_path, &settings, &api_key).await {
            eprintln!("Processing error: {:?}", e);
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
    let mut report = String::new();

    writeln!(report, "=== AutoTrim Report ===").ok();
    writeln!(report, "Video: {}", video_path).ok();
    writeln!(report, "Mode: {}", settings.mode).ok();
    writeln!(report, "Settings: silence_min={:.2}s, repetition_threshold={:.2}", settings.min_silence_duration, settings.repetition_threshold).ok();
    writeln!(report, "Remove silences: {}, Remove repetitions: {}", settings.remove_silences, settings.remove_repetitions).ok();
    writeln!(report).ok();

    // Get video info
    let video_info = ffmpeg::get_video_info(video_path)
        .context("Failed to get video info")?;

    let total_duration = video_info.duration;
    writeln!(report, "Video duration: {:.1}s ({:.1} min)", total_duration, total_duration / 60.0).ok();
    writeln!(report).ok();

    // Create temp directory for processing
    let temp_dir = std::env::temp_dir().join(format!("autotrim_{}", job_id));
    std::fs::create_dir_all(&temp_dir)?;

    let audio_path = temp_dir.join("audio.mp3");
    let audio_path_str = audio_path.to_str().unwrap();

    // Stage 1: Extract audio (0-20%)
    update_progress(job_id, "extracting", 5.0, None);
    ffmpeg::extract_audio(video_path, audio_path_str)?;
    update_progress(job_id, "extracting", 20.0, None);

    // Stage 2: Transcribe with Whisper (20-50%)
    update_progress(job_id, "transcribing", 25.0, None);
    let transcription = transcription::transcribe_audio(audio_path_str, api_key)
        .await
        .context("Failed to transcribe audio")?;
    update_progress(job_id, "transcribing", 50.0, None);

    writeln!(report, "--- Transcription ---").ok();
    writeln!(report, "Total words: {}", transcription.words.len()).ok();
    if let Some(first) = transcription.words.first() {
        writeln!(report, "First word: \"{}\" at {:.2}s", first.word, first.start).ok();
    }
    if let Some(last) = transcription.words.last() {
        writeln!(report, "Last word: \"{}\" at {:.2}s", last.word, last.end).ok();
    }
    writeln!(report).ok();

    let mut segments_to_remove: Vec<Segment> = Vec::new();

    // Stage 3: Detect silences from word timestamps (50-60%)
    let mut silences_removed = 0u32;
    if settings.remove_silences && !transcription.words.is_empty() {
        update_progress(job_id, "detecting_silences", 55.0, None);

        let words = &transcription.words;
        let padding = 0.15;

        writeln!(report, "--- Silence Detection ---").ok();
        writeln!(report, "Min silence duration: {:.2}s, Padding: {:.2}s", settings.min_silence_duration, padding).ok();
        writeln!(report).ok();

        // Silence before first word
        if words[0].start > settings.min_silence_duration + padding {
            let seg = Segment {
                start: 0.0,
                end: (words[0].start - padding).max(0.0),
            };
            writeln!(report, "SILENCE #{}: {:.2}s - {:.2}s (duration: {:.2}s) [before first word]",
                segments_to_remove.len() + 1, seg.start, seg.end, seg.end - seg.start).ok();
            segments_to_remove.push(seg);
        }

        // Silences between words
        for i in 0..words.len() - 1 {
            let gap_start = words[i].end;
            let gap_end = words[i + 1].start;
            let gap_duration = gap_end - gap_start;

            if gap_duration >= settings.min_silence_duration {
                let seg = Segment {
                    start: gap_start + padding,
                    end: (gap_end - padding).max(gap_start + padding),
                };
                writeln!(report, "SILENCE #{}: {:.2}s - {:.2}s (duration: {:.2}s) between \"{}\" and \"{}\"",
                    segments_to_remove.len() + 1, seg.start, seg.end, seg.end - seg.start,
                    words[i].word, words[i + 1].word).ok();
                segments_to_remove.push(seg);
            }
        }

        // Silence after last word
        let last_word_end = words.last().unwrap().end;
        if total_duration - last_word_end > settings.min_silence_duration + padding {
            let seg = Segment {
                start: last_word_end + padding,
                end: total_duration,
            };
            writeln!(report, "SILENCE #{}: {:.2}s - {:.2}s (duration: {:.2}s) [after last word]",
                segments_to_remove.len() + 1, seg.start, seg.end, seg.end - seg.start).ok();
            segments_to_remove.push(seg);
        }

        silences_removed = segments_to_remove.len() as u32;
        let total_silence_duration: f64 = segments_to_remove.iter().map(|s| s.end - s.start).sum();
        writeln!(report).ok();
        writeln!(report, "Total silences: {}, Total silence time: {:.1}s ({:.1} min)", silences_removed, total_silence_duration, total_silence_duration / 60.0).ok();
        writeln!(report).ok();

        update_progress(job_id, "detecting_silences", 60.0, None);
    } else {
        writeln!(report, "--- Silence Detection: SKIPPED ---").ok();
        writeln!(report).ok();
        update_progress(job_id, "detecting_silences", 60.0, None);
    }

    // Stage 4: Detect repetitions (60-70%)
    let mut repetitions_removed = 0u32;
    if settings.remove_repetitions {
        let phrases = transcription::segment_into_phrases(&transcription);
        writeln!(report, "--- Repetition Detection ---").ok();
        writeln!(report, "Phrases segmented: {}", phrases.len()).ok();
        writeln!(report, "Similarity threshold: {:.2}", settings.repetition_threshold).ok();
        writeln!(report).ok();

        if !phrases.is_empty() {
            update_progress(job_id, "detecting_repetitions", 65.0, None);
            let repetition_indices = transcription::detect_repetitions(
                &phrases,
                settings.repetition_threshold,
            );

            repetitions_removed = repetition_indices.len() as u32;

            for &idx in &repetition_indices {
                if let Some(phrase) = phrases.get(idx) {
                    writeln!(report, "REPETITION #{} (phrase {}): \"{}\"\n    Time: {:.2}s - {:.2}s (duration: {:.2}s)",
                        repetitions_removed, idx, phrase.text, phrase.start, phrase.end, phrase.end - phrase.start).ok();
                }
            }

            for idx in repetition_indices {
                if let Some(phrase) = phrases.get(idx) {
                    segments_to_remove.push(Segment {
                        start: phrase.start,
                        end: phrase.end,
                    });
                }
            }
        }

        writeln!(report).ok();
        writeln!(report, "Total repetitions removed: {}", repetitions_removed).ok();
        writeln!(report).ok();

        update_progress(job_id, "detecting_repetitions", 70.0, None);
    } else {
        writeln!(report, "--- Repetition Detection: SKIPPED ---").ok();
        writeln!(report).ok();
        update_progress(job_id, "detecting_repetitions", 70.0, None);
    }

    // Merge overlapping segments and create keep segments
    let segments_to_keep = calculate_keep_segments(&segments_to_remove, total_duration);

    // Calculate final duration
    let final_duration: f64 = segments_to_keep.iter()
        .map(|s| s.end - s.start)
        .sum();

    writeln!(report, "--- Summary ---").ok();
    writeln!(report, "Segments to remove: {} ({} silences + {} repetitions)", segments_to_remove.len(), silences_removed, repetitions_removed).ok();
    writeln!(report, "Segments to keep (after merge): {}", segments_to_keep.len()).ok();
    writeln!(report, "Original duration: {:.1}s ({:.1} min)", total_duration, total_duration / 60.0).ok();
    writeln!(report, "Final duration: {:.1}s ({:.1} min)", final_duration, final_duration / 60.0).ok();
    writeln!(report, "Time saved: {:.1}s ({:.1} min, {:.1}%)", total_duration - final_duration, (total_duration - final_duration) / 60.0, (1.0 - final_duration / total_duration) * 100.0).ok();
    writeln!(report).ok();

    // Write report file next to the input video
    let report_path = Path::new(video_path)
        .with_extension("autotrim_report.txt");
    if let Err(e) = std::fs::write(&report_path, &report) {
        eprintln!("Failed to write report: {}", e);
    } else {
        eprintln!("Report written to: {}", report_path.display());
    }

    // Stage 5: Render video (70-100%)
    update_progress(job_id, "rendering", 70.0, None);

    let output_path_hint = generate_output_path(video_path);
    let output_path = ffmpeg::render_video(
        video_path,
        &segments_to_keep,
        &output_path_hint,
        total_duration,
        &temp_dir,
        |progress| {
            let overall = 70.0 + progress * 29.0;
            update_progress(job_id, "rendering", overall, None);
        },
    )?;

    // Cleanup temp files
    let _ = std::fs::remove_dir_all(&temp_dir);

    let elapsed = start_time.elapsed();
    eprintln!("Processing complete in {:.1}s. Report: {}", elapsed.as_secs_f64(), report_path.display());

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
