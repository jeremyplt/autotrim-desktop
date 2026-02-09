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

/// Truncate a string to at most `max_chars` characters (UTF-8 safe).
fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingSettings {
    pub mode: String,
    pub remove_silences: bool,
    pub remove_repetitions: bool,
    pub silence_threshold_db: f64,
    pub min_silence_duration: f64,
    pub repetition_threshold: f64,
    #[serde(default = "default_transcription_provider")]
    pub transcription_provider: String,
}

fn default_transcription_provider() -> String {
    "whisper".to_string()
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
    #[allow(dead_code)]
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
    transcription_api_key: String,
    anthropic_api_key: String,
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
        if let Err(e) = process_video(&job_id_clone, &video_path, &settings, &transcription_api_key, &anthropic_api_key).await {
            eprintln!("Processing error: {:?}", e);
            if let Some(job) = JOBS.lock().unwrap().get_mut(&job_id_clone) {
                job.progress.stage = "error".to_string();
            }
        }
    });

    job_id
}

/// Save a debug JSON file next to the video for pipeline inspection.
fn save_debug_file(video_path: &str, step: &str, data: &impl Serialize) {
    let path = Path::new(video_path);
    let parent = path.parent().unwrap_or(Path::new(""));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
    let debug_path = parent.join(format!("{}_{}.json", stem, step));
    match serde_json::to_string_pretty(data) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&debug_path, &json) {
                eprintln!("Failed to write debug file {}: {}", debug_path.display(), e);
            } else {
                eprintln!("Debug file: {}", debug_path.display());
            }
        }
        Err(e) => eprintln!("Failed to serialize debug data for {}: {}", step, e),
    }
}

async fn process_video(
    job_id: &str,
    video_path: &str,
    settings: &ProcessingSettings,
    transcription_api_key: &str,
    anthropic_api_key: &str,
) -> Result<()> {
    let start_time = std::time::Instant::now();
    let mut report = String::new();

    writeln!(report, "=== AutoTrim Report ===").ok();
    writeln!(report, "Video: {}", video_path).ok();
    writeln!(report, "Mode: {}", settings.mode).ok();
    writeln!(report).ok();

    // Get video info
    let video_info = ffmpeg::get_video_info(video_path)
        .context("Failed to get video info")?;

    let total_duration = video_info.duration;
    let frame_rate = video_info.frame_rate;
    writeln!(report, "Duration: {:.1}s ({:.1} min)", total_duration, total_duration / 60.0).ok();
    writeln!(report).ok();

    // Create temp directory
    let temp_dir = std::env::temp_dir().join(format!("autotrim_{}", job_id));
    std::fs::create_dir_all(&temp_dir)?;

    let audio_path = temp_dir.join("audio.mp3");
    let audio_path_str = audio_path.to_str().unwrap();

    // --- Step 1: Extract audio ---
    update_progress(job_id, "extracting", 5.0, None);
    ffmpeg::extract_audio(video_path, audio_path_str)?;
    update_progress(job_id, "extracting", 15.0, None);

    // --- Step 2: Transcribe with Whisper ---
    update_progress(job_id, "transcribing", 20.0, None);
    let provider = transcription::TranscriptionProvider::from_str(&settings.transcription_provider);
    let transcription = transcription::transcribe_audio(audio_path_str, transcription_api_key, &provider)
        .await
        .context("Failed to transcribe audio")?;
    update_progress(job_id, "transcribing", 50.0, None);

    writeln!(report, "Words transcribed: {}", transcription.words.len()).ok();
    writeln!(report).ok();
    save_debug_file(video_path, "1_transcription", &transcription);

    // --- Step 3: Analyze transcript ---
    let mut silences_removed = 0u32;
    let mut repetitions_removed = 0u32;

    let segments_to_keep = if settings.remove_repetitions && !transcription.words.is_empty() {
        // === SIMPLIFIED AI PIPELINE ===
        // One Claude call handles everything: silence removal, retake detection,
        // false start removal, filler removal.
        update_progress(job_id, "analyzing", 52.0, None);

        // Segment into speech chunks (split on pauses > 0.5s + restart detection)
        let chunks = transcription::segment_into_chunks(&transcription.words, 0.5);
        save_debug_file(video_path, "2_chunks", &chunks);

        writeln!(report, "Speech chunks: {}", chunks.len()).ok();
        writeln!(report).ok();

        // ONE Claude call to determine which chunks to keep
        update_progress(job_id, "analyzing", 55.0, None);

        let keep_ids = transcription::determine_keep_ranges(
            &chunks, anthropic_api_key, &settings.mode
        ).await.context("AI transcript analysis failed")?;

        save_debug_file(video_path, "3_ai_keep_ids", &keep_ids);

        // Post-processing step 1: enforce retake groups (targeted version).
        // Only acts when Claude already removed some members of a group — confirming it IS a retake group.
        let keep_ids = enforce_retake_groups(&chunks, keep_ids);
        // Post-processing step 2: fix orphaned continuations.
        let keep_ids = fix_orphaned_continuations(&chunks, keep_ids);
        save_debug_file(video_path, "3b_postprocessed_keep_ids", &keep_ids);

        update_progress(job_id, "analyzing", 75.0, None);

        // Report what was kept/removed
        let keep_set: std::collections::HashSet<usize> = keep_ids.iter().copied().collect();
        writeln!(report, "--- AI Analysis ---").ok();
        for chunk in &chunks {
            let status = if keep_set.contains(&chunk.id) { "KEEP  " } else { "REMOVE" };
            let preview = truncate_str(&chunk.text, 80);
            writeln!(report, "{} [{}] {:.1}s-{:.1}s ({} words): \"{}\"",
                status, chunk.id, chunk.start, chunk.end, chunk.word_count, preview).ok();
        }
        writeln!(report).ok();

        repetitions_removed = (chunks.len() - keep_ids.len()) as u32;

        // Build keep ranges by walking through chunks in order.
        // Key: preserve natural gaps between consecutive KEPT chunks.
        // Only create a cut when Claude explicitly removed a chunk,
        // or when there's dead air (gap > max_keep_gap).
        let padding = match settings.mode.as_str() {
            "aggressive" => 0.08,
            "conservative" => 0.15,
            _ => 0.10,
        };
        let max_keep_gap = match settings.mode.as_str() {
            "aggressive" => 1.5,
            "conservative" => 4.0,
            _ => 2.5,  // moderate: preserve pauses up to 2.5s
        };

        let mut keep_ranges: Vec<Segment> = Vec::new();
        let mut range_start: Option<f64> = None;
        let mut range_end: f64 = 0.0;

        for chunk in &chunks {
            if keep_set.contains(&chunk.id) {
                match range_start {
                    Some(_) => {
                        let gap = chunk.start - range_end;
                        if gap > max_keep_gap {
                            // Dead air — save current range, start new one
                            keep_ranges.push(Segment {
                                start: (range_start.unwrap() - padding).max(0.0),
                                end: (range_end + padding).min(total_duration),
                            });
                            range_start = Some(chunk.start);
                        }
                        // else: extend current range (natural pause preserved)
                        range_end = chunk.end;
                    }
                    None => {
                        range_start = Some(chunk.start);
                        range_end = chunk.end;
                    }
                }
            } else {
                // Chunk removed by Claude — end current range (create a cut)
                if let Some(start) = range_start.take() {
                    keep_ranges.push(Segment {
                        start: (start - padding).max(0.0),
                        end: (range_end + padding).min(total_duration),
                    });
                }
            }
        }
        // Don't forget the last range
        if let Some(start) = range_start {
            keep_ranges.push(Segment {
                start: (start - padding).max(0.0),
                end: (range_end + padding).min(total_duration),
            });
        }

        let merged = merge_keep_segments(&keep_ranges);

        update_progress(job_id, "analyzing", 80.0, None);
        merged

    } else if settings.remove_silences && !transcription.words.is_empty() {
        // === SILENCE-ONLY PIPELINE (no AI needed) ===
        update_progress(job_id, "analyzing", 52.0, None);

        let words = transcription::filter_filler_words_contextual(&transcription.words, 0.3);
        let padding = match settings.mode.as_str() {
            "aggressive" => 0.12,
            "conservative" => 0.25,
            _ => 0.20,
        };

        let mut to_remove: Vec<Segment> = Vec::new();

        writeln!(report, "--- Silence Detection ---").ok();
        writeln!(report, "Min silence: {:.2}s, Padding: {:.2}s", settings.min_silence_duration, padding).ok();
        writeln!(report).ok();

        // Silence before first word
        if words[0].start > settings.min_silence_duration + padding {
            to_remove.push(Segment {
                start: 0.0,
                end: (words[0].start - padding).max(0.0),
            });
        }

        // Silences between words
        for i in 0..words.len() - 1 {
            let gap = words[i + 1].start - words[i].end;
            if gap >= settings.min_silence_duration {
                to_remove.push(Segment {
                    start: words[i].end + padding,
                    end: (words[i + 1].start - padding).max(words[i].end + padding),
                });
            }
        }

        // Silence after last word
        let last_end = words.last().unwrap().end;
        if total_duration - last_end > settings.min_silence_duration + padding {
            to_remove.push(Segment {
                start: last_end + padding,
                end: total_duration,
            });
        }

        silences_removed = to_remove.len() as u32;
        writeln!(report, "Silences found: {}", silences_removed).ok();
        writeln!(report).ok();

        update_progress(job_id, "analyzing", 80.0, None);
        calculate_keep_segments(&to_remove, total_duration, &settings.mode)

    } else {
        update_progress(job_id, "analyzing", 80.0, None);
        vec![Segment { start: 0.0, end: total_duration }]
    };

    save_debug_file(video_path, "4_keep_segments", &segments_to_keep);

    // Summary
    let final_duration: f64 = segments_to_keep.iter()
        .map(|s| s.end - s.start)
        .sum();

    writeln!(report, "--- Summary ---").ok();
    writeln!(report, "Keep segments: {}", segments_to_keep.len()).ok();
    writeln!(report, "Original: {:.1}s ({:.1} min)", total_duration, total_duration / 60.0).ok();
    writeln!(report, "Final: {:.1}s ({:.1} min)", final_duration, final_duration / 60.0).ok();
    writeln!(report, "Saved: {:.1}s ({:.1}%)",
        total_duration - final_duration,
        (1.0 - final_duration / total_duration) * 100.0).ok();
    writeln!(report).ok();

    // Write report file
    let report_path = Path::new(video_path)
        .with_extension("autotrim_report.txt");
    if let Err(e) = std::fs::write(&report_path, &report) {
        eprintln!("Failed to write report: {}", e);
    } else {
        eprintln!("Report: {}", report_path.display());
    }

    // --- Step 4: Render final video ---
    update_progress(job_id, "rendering", 80.0, None);

    let output_path_hint = generate_output_path(video_path);
    let output_path = ffmpeg::render_video(
        video_path,
        &segments_to_keep,
        &output_path_hint,
        total_duration,
        &temp_dir,
        |progress| {
            let overall = 80.0 + progress * 19.0;
            update_progress(job_id, "rendering", overall, None);
        },
        frame_rate,
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

/// Merge overlapping or adjacent keep segments, filtering out invalid ones.
fn merge_keep_segments(segments: &[Segment]) -> Vec<Segment> {
    if segments.is_empty() {
        return Vec::new();
    }

    // Filter out segments with negative or zero duration (from non-monotonic timestamps)
    let valid: Vec<Segment> = segments.iter()
        .filter(|s| s.end > s.start)
        .cloned()
        .collect();

    if valid.is_empty() {
        return Vec::new();
    }

    let mut sorted = valid;
    sorted.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());

    let mut merged = vec![sorted[0].clone()];

    for seg in sorted.iter().skip(1) {
        let last = merged.last_mut().unwrap();
        if seg.start <= last.end {
            last.end = last.end.max(seg.end);
        } else {
            merged.push(seg.clone());
        }
    }

    merged
}

/// Post-process: if a kept chunk starts with a lowercase letter (= sentence continuation),
/// ensure its predecessor chunk is also kept. Without it, the continuation is orphaned
/// (e.g., chunk "reste un outil incroyable" makes no sense without the preceding "Cloud Code ça").
fn fix_orphaned_continuations(chunks: &[transcription::SpeechChunk], mut keep_ids: Vec<usize>) -> Vec<usize> {
    let keep_set: std::collections::HashSet<usize> = keep_ids.iter().copied().collect();
    let mut to_add = Vec::new();

    for chunk in chunks {
        if !keep_set.contains(&chunk.id) || chunk.id == 0 {
            continue;
        }

        let first_char = chunk.text.chars().next().unwrap_or('A');
        if first_char.is_lowercase() {
            let prev_id = chunk.id - 1;
            if !keep_set.contains(&prev_id) {
                eprintln!("Post-process: restoring chunk {} (predecessor of continuation chunk {} \"{}\")",
                    prev_id, chunk.id, truncate_str(&chunk.text, 40));
                to_add.push(prev_id);
            }
        }
    }

    for id in to_add {
        if !keep_ids.contains(&id) {
            keep_ids.push(id);
        }
    }
    keep_ids.sort();
    keep_ids
}

/// Post-process: enforce retake group rules with TARGETED conditions.
/// Only acts when Claude already removed at least 1 member of the group — confirming it IS a retake.
/// This avoids the false positive problem of the old version (which would create groups from
/// common French openers used in different contexts).
fn enforce_retake_groups(chunks: &[transcription::SpeechChunk], mut keep_ids: Vec<usize>) -> Vec<usize> {
    use std::collections::HashMap;

    let min_match = 3;
    let max_time_span = 180.0;
    let max_gap_between_members = 60.0;

    let normalize = |s: &str| -> String {
        s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
    };

    // Group chunks by 3-word opener
    let mut opener_groups: HashMap<Vec<String>, Vec<usize>> = HashMap::new();
    for chunk in chunks {
        if chunk.word_count < min_match {
            continue;
        }
        let words: Vec<String> = chunk.text.split_whitespace()
            .take(min_match)
            .map(|w| normalize(&w))
            .collect();
        if words.len() == min_match {
            opener_groups.entry(words).or_default().push(chunk.id);
        }
    }

    let keep_set: std::collections::HashSet<usize> = keep_ids.iter().copied().collect();

    for (opener, ids) in &opener_groups {
        if ids.len() < 2 {
            continue;
        }

        // Split into sub-groups based on max_gap_between_members
        let mut sub_groups: Vec<Vec<usize>> = Vec::new();
        let mut current_sub: Vec<usize> = vec![ids[0]];

        for pair in ids.windows(2) {
            if let (Some(a), Some(b)) = (chunks.get(pair[0]), chunks.get(pair[1])) {
                if b.start - a.end > max_gap_between_members {
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

            // Check time span
            let first = chunks.get(*sub_ids.first().unwrap());
            let last = chunks.get(*sub_ids.last().unwrap());
            if let (Some(f), Some(l)) = (first, last) {
                if l.end - f.start > max_time_span {
                    continue;
                }
            }

            // CRITICAL: Require content overlap (≥3 shared content words between any pair)
            let mut has_content_overlap = false;
            'check: for (ai, &id_a) in sub_ids.iter().enumerate() {
                for &id_b in &sub_ids[ai + 1..] {
                    if let (Some(a), Some(b)) = (chunks.get(id_a), chunks.get(id_b)) {
                        let shared = transcription::analysis::count_shared_content_words(&a.text, &b.text);
                        if shared >= 3 {
                            has_content_overlap = true;
                            break 'check;
                        }
                    }
                }
            }
            if !has_content_overlap {
                continue;
            }

            // How many from this group did Claude keep vs remove?
            let kept_from_group: Vec<usize> = sub_ids.iter()
                .filter(|id| keep_set.contains(id))
                .copied()
                .collect();
            let removed_from_group: Vec<usize> = sub_ids.iter()
                .filter(|id| !keep_set.contains(id))
                .copied()
                .collect();

            // TARGETED CONDITION: Claude must have already removed at least 1 member.
            // This confirms the group IS a retake sequence (not just common French openers).
            if removed_from_group.is_empty() {
                continue; // Claude kept everything — it decided these are NOT retakes
            }

            if kept_from_group.len() > 1 {
                // Claude confirmed some are retakes but missed others.
                // Keep only the last member of the group.
                let mandated_keep = *sub_ids.last().unwrap();
                let removed_count = kept_from_group.iter().filter(|&&id| id != mandated_keep).count();
                eprintln!("Post-process: retake group {:?} (opener: \"{}\") has {} kept/{} removed, enforcing keep only [{}] (-{} chunks)",
                    sub_ids, opener.join(" "), kept_from_group.len(), removed_from_group.len(), mandated_keep, removed_count);

                for &id in &kept_from_group {
                    if id != mandated_keep {
                        keep_ids.retain(|&x| x != id);
                    }
                }
            }
        }
    }

    keep_ids.sort();
    keep_ids.dedup();
    keep_ids
}

/// Calculate keep segments from removal segments (used by silence-only pipeline).
fn calculate_keep_segments(to_remove: &[Segment], total_duration: f64, mode: &str) -> Vec<Segment> {
    if to_remove.is_empty() {
        return vec![Segment { start: 0.0, end: total_duration }];
    }

    let (merge_gap, min_keep_duration) = match mode {
        "aggressive" => (1.5, 1.2),
        "conservative" => (0.5, 0.3),
        _ => (1.0, 0.8),
    };

    let mut sorted = to_remove.to_vec();
    sorted.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());

    let mut merged: Vec<Segment> = Vec::new();
    let mut current = sorted[0].clone();

    for segment in sorted.iter().skip(1) {
        if segment.start <= current.end + merge_gap {
            current.end = current.end.max(segment.end);
        } else {
            merged.push(current);
            current = segment.clone();
        }
    }
    merged.push(current);

    let mut keep_segments = Vec::new();
    let mut last_end = 0.0;

    for segment in &merged {
        if segment.start > last_end {
            keep_segments.push(Segment {
                start: last_end,
                end: segment.start,
            });
        }
        last_end = segment.end;
    }

    if last_end < total_duration {
        keep_segments.push(Segment {
            start: last_end,
            end: total_duration,
        });
    }

    keep_segments.retain(|s| s.end - s.start >= min_keep_duration);

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
