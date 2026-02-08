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
    openai_api_key: String,
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
        if let Err(e) = process_video(&job_id_clone, &video_path, &settings, &openai_api_key, &anthropic_api_key).await {
            eprintln!("Processing error: {:?}", e);
            if let Some(job) = JOBS.lock().unwrap().get_mut(&job_id_clone) {
                job.progress.stage = "error".to_string();
            }
        }
    });

    job_id
}

/// Detect sparse regions where speech density is too low (setup time, keyboard noise, etc.).
/// Also detects Whisper hallucination zones where the same phrases repeat endlessly.
/// Uses a sliding window to count meaningful words per minute.
fn detect_sparse_regions(
    words: &[transcription::Word],
    mode: &str,
    total_duration: f64,
) -> Vec<Segment> {
    if words.is_empty() || total_duration < 60.0 {
        return Vec::new();
    }

    let min_words_per_minute = match mode {
        "aggressive" => 8.0,
        "conservative" => 3.0,
        _ => 5.0, // moderate
    };

    let window_size = 60.0; // 60 second windows
    let step_size = 30.0;   // 30 second steps
    let padding = 0.3;

    // Filter filler words for density calculation
    let meaningful_words = transcription::filter_filler_words(words);

    let mut sparse_windows: Vec<(f64, f64)> = Vec::new();
    let mut window_start = 0.0;

    while window_start + window_size <= total_duration {
        let window_end = window_start + window_size;

        // Count meaningful words in this window
        let word_count = meaningful_words.iter()
            .filter(|w| w.start >= window_start && w.end <= window_end)
            .count();

        let words_per_minute = word_count as f64; // window is 60s = 1 minute

        if words_per_minute < min_words_per_minute {
            sparse_windows.push((window_start, window_end));
        }

        window_start += step_size;
    }

    // Also detect Whisper hallucination zones: same 3-gram repeating excessively
    // Whisper sometimes loops the same phrase when it can't understand audio
    let hallucination_regions = detect_whisper_hallucinations(&meaningful_words);
    for region in &hallucination_regions {
        eprintln!("Whisper hallucination detected: {:.1}s - {:.1}s", region.0, region.1);
        sparse_windows.push(*region);
    }

    if sparse_windows.is_empty() {
        return Vec::new();
    }

    // Sort and merge overlapping/adjacent sparse windows into regions
    sparse_windows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    let mut regions: Vec<Segment> = Vec::new();
    let mut current_start = sparse_windows[0].0;
    let mut current_end = sparse_windows[0].1;

    for &(ws, we) in sparse_windows.iter().skip(1) {
        if ws <= current_end {
            current_end = current_end.max(we);
        } else {
            regions.push(Segment {
                start: (current_start + padding).max(0.0),
                end: (current_end - padding).max(current_start + padding),
            });
            current_start = ws;
            current_end = we;
        }
    }
    regions.push(Segment {
        start: (current_start + padding).max(0.0),
        end: (current_end - padding).max(current_start + padding),
    });

    // Filter out very short regions (< 10s after padding) - probably not real dead zones
    regions.retain(|r| r.end - r.start >= 10.0);

    regions
}

/// Detect Whisper hallucination zones where the same 3-gram repeats excessively.
/// Returns (start, end) time ranges of hallucination zones.
fn detect_whisper_hallucinations(words: &[transcription::Word]) -> Vec<(f64, f64)> {
    use std::collections::HashMap;

    if words.len() < 10 {
        return Vec::new();
    }

    let window_size = 60.0; // 60 second analysis window
    let step_size = 30.0;
    let min_repeat_ratio = 0.7; // 70% of words in repeated 3-grams = hallucination
    let min_trigram_repeats = 5; // a 3-gram must appear 5+ times to count as repetitive

    let mut hallucination_windows: Vec<(f64, f64)> = Vec::new();

    let first_start = words.first().map(|w| w.start).unwrap_or(0.0);
    let last_end = words.last().map(|w| w.end).unwrap_or(0.0);

    let mut win_start = first_start;
    while win_start + window_size <= last_end + step_size {
        let win_end = win_start + window_size;

        // Get words in this window
        let win_words: Vec<&str> = words.iter()
            .filter(|w| w.start >= win_start && w.end <= win_end)
            .map(|w| w.word.as_str())
            .collect();

        if win_words.len() >= 9 {
            // Count 3-grams
            let mut trigram_counts: HashMap<String, usize> = HashMap::new();
            for i in 0..win_words.len() - 2 {
                let tri = format!("{} {} {}", win_words[i], win_words[i+1], win_words[i+2]);
                *trigram_counts.entry(tri).or_insert(0) += 1;
            }

            // Count words that are part of highly-repeated 3-grams
            let mut repeated_positions = std::collections::HashSet::new();
            for i in 0..win_words.len() - 2 {
                let tri = format!("{} {} {}", win_words[i], win_words[i+1], win_words[i+2]);
                if trigram_counts.get(&tri).copied().unwrap_or(0) >= min_trigram_repeats {
                    repeated_positions.insert(i);
                    repeated_positions.insert(i + 1);
                    repeated_positions.insert(i + 2);
                }
            }

            let ratio = repeated_positions.len() as f64 / win_words.len() as f64;
            if ratio >= min_repeat_ratio {
                hallucination_windows.push((win_start, win_end));
            }
        }

        win_start += step_size;
    }

    // Merge adjacent windows
    if hallucination_windows.is_empty() {
        return Vec::new();
    }

    let mut merged: Vec<(f64, f64)> = Vec::new();
    let mut cur_start = hallucination_windows[0].0;
    let mut cur_end = hallucination_windows[0].1;

    for &(ws, we) in hallucination_windows.iter().skip(1) {
        if ws <= cur_end {
            cur_end = cur_end.max(we);
        } else {
            merged.push((cur_start, cur_end));
            cur_start = ws;
            cur_end = we;
        }
    }
    merged.push((cur_start, cur_end));

    merged
}

/// Detect words with abnormally long durations — Whisper hides repeated/skipped content
/// by stretching a single word's timestamp to cover the full duration.
/// Example: "un" mapped to 2.26s when the audio actually contains "un outil incroyable un outil incroyable".
/// Returns removal segments for the hidden content (keeping the actual word at the start).
fn detect_stretched_words(words: &[transcription::Word]) -> Vec<Segment> {
    let mut anomalies = Vec::new();
    let keep_duration = 0.3; // keep the first 0.3s (the actual word)

    for word in words {
        let duration = word.end - word.start;
        let char_count = word.word.chars().count();

        // Expected max duration based on word length: short words = 0.5s max, longer words = more
        let max_expected = (char_count as f64 * 0.08).max(0.5).min(1.5);

        // Flag if actual duration is way longer than expected
        // Must be > 1.5s absolute AND > expected + 1.0s
        if duration > 1.5 && duration > max_expected + 1.0 {
            let cut_start = word.start + keep_duration;
            if cut_start < word.end {
                eprintln!("Stretched word: \"{}\" ({:.2}s-{:.2}s, duration: {:.2}s, expected max: {:.2}s)",
                    word.word, word.start, word.end, duration, max_expected);
                anomalies.push(Segment {
                    start: cut_start,
                    end: word.end,
                });
            }
        }
    }

    anomalies
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
    openai_api_key: &str,
    anthropic_api_key: &str,
) -> Result<()> {
    let start_time = std::time::Instant::now();
    let mut report = String::new();

    writeln!(report, "=== AutoTrim Report ===").ok();
    writeln!(report, "Video: {}", video_path).ok();
    writeln!(report, "Mode: {}", settings.mode).ok();
    writeln!(report, "Settings: silence_min={:.2}s", settings.min_silence_duration).ok();
    writeln!(report, "Remove silences: {}, Remove repetitions: {}", settings.remove_silences, settings.remove_repetitions).ok();
    writeln!(report).ok();

    // Get video info
    let video_info = ffmpeg::get_video_info(video_path)
        .context("Failed to get video info")?;

    let total_duration = video_info.duration;
    let frame_rate = video_info.frame_rate;
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
    let transcription = transcription::transcribe_audio(audio_path_str, openai_api_key)
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

    // DEBUG: Save transcription
    save_debug_file(video_path, "1_transcription", &transcription);

    let mut segments_to_remove: Vec<Segment> = Vec::new();

    // Stage 3: Detect silences + sparse regions (50-57%)
    let mut silences_removed = 0u32;
    let mut sparse_regions_removed = 0u32;
    let mut silence_segments: Vec<Segment> = Vec::new();
    if settings.remove_silences && !transcription.words.is_empty() {
        update_progress(job_id, "detecting_silences", 52.0, None);

        // Filter out filler words so "euh", "hum" etc don't break silence gaps
        let words = transcription::filter_filler_words(&transcription.words);
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
            segments_to_remove.push(seg.clone());
            silence_segments.push(seg);
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
                segments_to_remove.push(seg.clone());
                silence_segments.push(seg);
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
            segments_to_remove.push(seg.clone());
            silence_segments.push(seg);
        }

        silences_removed = silence_segments.len() as u32;
        let total_silence_duration: f64 = silence_segments.iter().map(|s| s.end - s.start).sum();
        writeln!(report).ok();
        writeln!(report, "Total silences: {}, Total silence time: {:.1}s ({:.1} min)",
            silences_removed, total_silence_duration, total_silence_duration / 60.0).ok();
        writeln!(report).ok();

        // DEBUG: Save silences
        save_debug_file(video_path, "2_silences", &silence_segments);

        // Sparse region detection (dead zones with keyboard noise, setup, etc.)
        update_progress(job_id, "detecting_silences", 55.0, None);

        let sparse_regions = detect_sparse_regions(&transcription.words, &settings.mode, total_duration);
        // DEBUG: Save sparse regions
        save_debug_file(video_path, "3_sparse_regions", &sparse_regions);

        if !sparse_regions.is_empty() {
            writeln!(report, "--- Sparse Region Detection (Dead Zones) ---").ok();
            for (i, region) in sparse_regions.iter().enumerate() {
                writeln!(report, "DEAD ZONE #{}: {:.1}s - {:.1}s (duration: {:.1}s, {:.1} min)",
                    i + 1, region.start, region.end, region.end - region.start,
                    (region.end - region.start) / 60.0).ok();
            }
            sparse_regions_removed = sparse_regions.len() as u32;
            let total_sparse_duration: f64 = sparse_regions.iter().map(|s| s.end - s.start).sum();
            writeln!(report, "Total dead zones: {}, Total time: {:.1}s ({:.1} min)",
                sparse_regions_removed, total_sparse_duration, total_sparse_duration / 60.0).ok();
            writeln!(report).ok();
            segments_to_remove.extend(sparse_regions);
        }

        // Detect Whisper timestamp anomalies: words with abnormally long durations
        // that hide repeated/skipped content (e.g., "un" lasting 2.26s = hidden repetition)
        let whisper_anomalies = detect_stretched_words(&transcription.words);
        if !whisper_anomalies.is_empty() {
            writeln!(report, "--- Whisper Timestamp Anomalies ---").ok();
            for (i, seg) in whisper_anomalies.iter().enumerate() {
                writeln!(report, "STRETCHED #{}: {:.2}s - {:.2}s (hidden content: {:.1}s)",
                    i + 1, seg.start, seg.end, seg.end - seg.start).ok();
            }
            writeln!(report, "Total anomalies: {}", whisper_anomalies.len()).ok();
            writeln!(report).ok();
            segments_to_remove.extend(whisper_anomalies);
        }

        update_progress(job_id, "detecting_silences", 57.0, None);
    } else {
        writeln!(report, "--- Silence Detection: SKIPPED ---").ok();
        writeln!(report).ok();
        update_progress(job_id, "detecting_silences", 57.0, None);
    }

    // Stage 4: Detect retakes with AI (57-80%)
    let mut repetitions_removed = 0u32;
    if settings.remove_repetitions {
        // 4a: Segment into passages, skipping already-removed segments
        update_progress(job_id, "analyzing_retakes", 58.0, None);

        let passages = transcription::segment_into_passages(&transcription.words, &segments_to_remove);

        writeln!(report, "--- Retake Detection (AI - Claude Sonnet) ---").ok();
        writeln!(report, "Passages segmented: {} (from {} words, after écremage)", passages.len(), transcription.words.len()).ok();
        writeln!(report, "Mode: {}", settings.mode).ok();
        writeln!(report).ok();

        // DEBUG: Save passages
        save_debug_file(video_path, "4_passages", &passages);

        if !passages.is_empty() {
            // 4b: Detect retakes via Claude
            update_progress(job_id, "analyzing_retakes", 60.0, None);

            match transcription::detect_retakes(&passages, anthropic_api_key, &settings.mode).await {
                Ok(detection_result) => {
                    // DEBUG: Save retake groups
                    save_debug_file(video_path, "5_retake_groups", &detection_result);

                    let num_groups = detection_result.retake_groups.len();
                    let num_abandoned = detection_result.abandoned_passages.len();
                    writeln!(report, "Retake groups identified: {}", num_groups).ok();
                    writeln!(report, "Abandoned passages identified: {}", num_abandoned).ok();
                    writeln!(report).ok();

                    for group in &detection_result.retake_groups {
                        writeln!(report, "GROUP #{}: \"{}\" (confidence: {})",
                            group.group_id, group.description, group.confidence).ok();
                        for &id in &group.keep {
                            if let Some(p) = passages.get(id) {
                                let preview = truncate_str(&p.text, 80);
                                writeln!(report, "  KEEP [{}]: \"{}\"", id, preview).ok();
                            }
                        }
                        for &id in &group.remove {
                            if let Some(p) = passages.get(id) {
                                let preview = truncate_str(&p.text, 80);
                                writeln!(report, "  REMOVE [{}]: \"{}\"", id, preview).ok();
                            }
                        }
                        writeln!(report).ok();
                    }

                    for ap in &detection_result.abandoned_passages {
                        if let Some(p) = passages.get(ap.id) {
                            let preview = truncate_str(&p.text, 80);
                            writeln!(report, "ABANDONED [{}] ({}): \"{}\" - {}",
                                ap.id, ap.confidence, preview, ap.reason).ok();
                        }
                    }
                    writeln!(report).ok();

                    // Apply abandoned passages (filtered by confidence based on mode)
                    let abandoned_ids: Vec<usize> = detection_result.abandoned_passages.iter()
                        .filter(|a| match settings.mode.as_str() {
                            "aggressive" => true,
                            "conservative" => a.confidence == "high",
                            _ => a.confidence == "high" || a.confidence == "medium",
                        })
                        .map(|a| a.id)
                        .collect();

                    for &id in &abandoned_ids {
                        if let Some(passage) = passages.get(id) {
                            segments_to_remove.push(Segment {
                                start: passage.start,
                                end: passage.end,
                            });
                            repetitions_removed += 1;
                        }
                    }

                    let has_retake_groups = !detection_result.retake_groups.is_empty();
                    if has_retake_groups {
                        // 4c: Verify retakes via Claude
                        update_progress(job_id, "verifying_cuts", 72.0, None);

                        match transcription::verify_retakes(
                            &passages,
                            &detection_result.retake_groups,
                            anthropic_api_key,
                            &settings.mode,
                        ).await {
                            Ok((passage_ids_to_remove, verifications)) => {
                                // DEBUG: Save verification
                                save_debug_file(video_path, "6_verification", &verifications);

                                writeln!(report, "--- Verification Results ---").ok();
                                for v in &verifications {
                                    let status = if v.approved { "APPROVED" } else { "REJECTED" };
                                    writeln!(report, "Group #{}: {} - {}", v.group_id, status, v.reason).ok();
                                }
                                writeln!(report).ok();

                                repetitions_removed += passage_ids_to_remove.len() as u32;

                                // Convert passage IDs to time segments
                                for &id in &passage_ids_to_remove {
                                    if let Some(passage) = passages.get(id) {
                                        segments_to_remove.push(Segment {
                                            start: passage.start,
                                            end: passage.end,
                                        });
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Retake verification failed: {:?}", e);
                                writeln!(report, "Retake verification failed: {}", e).ok();
                                writeln!(report, "Falling back: using all detected retakes without verification").ok();

                                // Fallback: use all detected retakes without verification
                                for group in &detection_result.retake_groups {
                                    for &id in &group.remove {
                                        if let Some(passage) = passages.get(id) {
                                            segments_to_remove.push(Segment {
                                                start: passage.start,
                                                end: passage.end,
                                            });
                                            repetitions_removed += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("AI retake detection failed: {:?}", e);
                    writeln!(report, "AI retake detection failed: {}", e).ok();
                }
            }
        }

        writeln!(report).ok();
        writeln!(report, "Total retake passages removed: {}", repetitions_removed).ok();
        writeln!(report).ok();

        update_progress(job_id, "verifying_cuts", 80.0, None);
    } else {
        writeln!(report, "--- Retake Detection: SKIPPED ---").ok();
        writeln!(report).ok();
        update_progress(job_id, "verifying_cuts", 80.0, None);
    }

    // Merge overlapping segments and create keep segments
    let segments_to_keep = calculate_keep_segments(&segments_to_remove, total_duration);

    // DEBUG: Save final segments
    save_debug_file(video_path, "7_final_keep_segments", &segments_to_keep);

    // Calculate final duration
    let final_duration: f64 = segments_to_keep.iter()
        .map(|s| s.end - s.start)
        .sum();

    writeln!(report, "--- Summary ---").ok();
    writeln!(report, "Segments to remove: {} ({} silences + {} dead zones + {} retakes)",
        segments_to_remove.len(), silences_removed, sparse_regions_removed, repetitions_removed).ok();
    writeln!(report, "Segments to keep (after merge): {}", segments_to_keep.len()).ok();
    writeln!(report, "Original duration: {:.1}s ({:.1} min)", total_duration, total_duration / 60.0).ok();
    writeln!(report, "Final duration: {:.1}s ({:.1} min)", final_duration, final_duration / 60.0).ok();
    writeln!(report, "Time saved: {:.1}s ({:.1} min, {:.1}%)",
        total_duration - final_duration,
        (total_duration - final_duration) / 60.0,
        (1.0 - final_duration / total_duration) * 100.0).ok();
    writeln!(report).ok();

    // Write report file next to the input video
    let report_path = Path::new(video_path)
        .with_extension("autotrim_report.txt");
    if let Err(e) = std::fs::write(&report_path, &report) {
        eprintln!("Failed to write report: {}", e);
    } else {
        eprintln!("Report written to: {}", report_path.display());
    }

    // Stage 5: Render video (80-100%)
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

fn calculate_keep_segments(to_remove: &[Segment], total_duration: f64) -> Vec<Segment> {
    if to_remove.is_empty() {
        return vec![Segment { start: 0.0, end: total_duration }];
    }

    // Sort segments by start time
    let mut sorted = to_remove.to_vec();
    sorted.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());

    // Merge overlapping segments AND segments separated by < 0.5s.
    // A tiny gap between two removals is just a mouth click or breath — remove it too.
    let merge_gap = 0.5;
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

    // Create keep segments (inverse of remove segments)
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

    // Add final segment if needed
    if last_end < total_duration {
        keep_segments.push(Segment {
            start: last_end,
            end: total_duration,
        });
    }

    // Remove micro-fragments: any keep segment < 0.3s is just noise, absorb into removals
    let min_keep_duration = 0.3;
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
