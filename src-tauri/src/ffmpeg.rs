use std::process::Command;
use regex::Regex;
use serde::{Deserialize, Serialize};
use anyhow::{Result, Context};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VideoMetadata {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
}

pub fn check_ffmpeg_installed() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok()
}

pub fn get_video_info(path: &str) -> Result<VideoMetadata> {
    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=width,height,duration",
            "-of", "json",
            path
        ])
        .output()
        .context("Failed to run ffprobe")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // Parse JSON output
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .context("Failed to parse ffprobe output")?;
    
    let stream = json["streams"][0].as_object()
        .context("No video stream found")?;
    
    let duration_str = stream.get("duration")
        .and_then(|v| v.as_str())
        .context("No duration found")?;
    
    let duration = duration_str.parse::<f64>()
        .context("Failed to parse duration")?;
    
    let width = stream.get("width")
        .and_then(|v| v.as_u64())
        .context("No width found")? as u32;
    
    let height = stream.get("height")
        .and_then(|v| v.as_u64())
        .context("No height found")? as u32;
    
    Ok(VideoMetadata {
        duration,
        width,
        height,
    })
}

pub fn extract_audio(video_path: &str, output_path: &str) -> Result<()> {
    // Extract as mp3 mono 64kbps to stay under OpenAI's 25MB limit
    let status = Command::new("ffmpeg")
        .args([
            "-i", video_path,
            "-vn",
            "-ac", "1",
            "-ar", "16000",
            "-b:a", "64k",
            "-y",
            output_path
        ])
        .status()
        .context("Failed to run ffmpeg for audio extraction")?;

    if !status.success() {
        anyhow::bail!("FFmpeg audio extraction failed");
    }

    Ok(())
}

pub fn detect_silences(
    audio_path: &str,
    threshold_db: f64,
    min_duration: f64
) -> Result<Vec<Segment>> {
    let output = Command::new("ffmpeg")
        .args([
            "-i", audio_path,
            "-af", &format!("silencedetect=n={}dB:d={}", threshold_db, min_duration),
            "-f", "null",
            "-"
        ])
        .output()
        .context("Failed to run ffmpeg for silence detection")?;
    
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_silence_output(&stderr)
}

fn parse_silence_output(output: &str) -> Result<Vec<Segment>> {
    let start_regex = Regex::new(r"silence_start: ([\d.]+)").unwrap();
    let end_regex = Regex::new(r"silence_end: ([\d.]+)").unwrap();
    
    let mut segments = Vec::new();
    let mut current_start: Option<f64> = None;
    
    for line in output.lines() {
        if let Some(caps) = start_regex.captures(line) {
            if let Ok(start) = caps[1].parse::<f64>() {
                current_start = Some(start);
            }
        } else if let Some(caps) = end_regex.captures(line) {
            if let Ok(end) = caps[1].parse::<f64>() {
                if let Some(start) = current_start.take() {
                    segments.push(Segment { start, end });
                }
            }
        }
    }
    
    Ok(segments)
}

pub fn render_video(
    input_path: &str,
    segments_to_keep: &[Segment],
    output_path: &str,
    _total_duration: f64,
    temp_dir: &std::path::Path,
    on_progress: impl Fn(f64),
) -> Result<String> {
    if segments_to_keep.is_empty() {
        anyhow::bail!("No segments to keep");
    }

    let total_segments = segments_to_keep.len();
    eprintln!("Rendering {} segments via stream copy + concat", total_segments);

    on_progress(0.0);

    // Phase 1: Extract each segment as a separate .mp4 file using stream copy
    let mut segment_files = Vec::new();
    for (i, segment) in segments_to_keep.iter().enumerate() {
        let seg_path = temp_dir.join(format!("seg_{:04}.mp4", i));
        let seg_path_str = seg_path.to_str().unwrap().to_string();
        let duration = segment.end - segment.start;

        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-ss", &format!("{:.3}", segment.start),
                "-i", input_path,
                "-t", &format!("{:.3}", duration),
                "-c", "copy",
                "-avoid_negative_ts", "make_zero",
                "-map", "0:v:0",
                "-map", "0:a:0",
                &seg_path_str,
            ])
            .output()
            .context(format!("Failed to extract segment {}", i))?;

        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            eprintln!("Warning: segment {} extraction failed: {}", i, stderr);
            continue;
        }

        segment_files.push(seg_path_str);

        let progress = (i + 1) as f64 / total_segments as f64 * 0.8;
        on_progress(progress);
    }

    if segment_files.is_empty() {
        anyhow::bail!("No segments were extracted successfully");
    }

    // Phase 2: Write concat list
    let concat_path = temp_dir.join("concat.txt");
    let concat_content: String = segment_files.iter()
        .map(|p| format!("file '{}'", p))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&concat_path, &concat_content)
        .context("Failed to write concat list")?;

    on_progress(0.85);

    // Phase 3: Concatenate all segments with stream copy
    let output_mp4 = std::path::Path::new(output_path)
        .with_extension("mp4")
        .to_str()
        .unwrap()
        .to_string();

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f", "concat",
            "-safe", "0",
            "-i", concat_path.to_str().unwrap(),
            "-c", "copy",
            "-movflags", "+faststart",
            &output_mp4,
        ])
        .status()
        .context("Failed to run ffmpeg for concat")?;

    if !status.success() {
        anyhow::bail!("FFmpeg concat failed");
    }

    on_progress(1.0);
    Ok(output_mp4)
}

pub fn get_file_size(path: &str) -> Result<u64> {
    let metadata = std::fs::metadata(path)
        .context("Failed to get file metadata")?;
    Ok(metadata.len())
}
