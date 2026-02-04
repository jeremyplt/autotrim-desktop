use std::process::Command;
use std::path::Path;
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
    let status = Command::new("ffmpeg")
        .args([
            "-i", video_path,
            "-vn",
            "-acodec", "pcm_s16le",
            "-ar", "16000",
            "-ac", "1",
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
    total_duration: f64,
) -> Result<()> {
    if segments_to_keep.is_empty() {
        anyhow::bail!("No segments to keep");
    }
    
    // Create filter complex for concatenation
    let mut filter_parts = Vec::new();
    
    for (i, segment) in segments_to_keep.iter().enumerate() {
        let duration = segment.end - segment.start;
        filter_parts.push(format!(
            "[0:v]trim=start={}:end={},setpts=PTS-STARTPTS[v{}];[0:a]atrim=start={}:end={},asetpts=PTS-STARTPTS[a{}]",
            segment.start, segment.end, i,
            segment.start, segment.end, i
        ));
    }
    
    // Concatenate all segments
    let v_concat = (0..segments_to_keep.len())
        .map(|i| format!("[v{}]", i))
        .collect::<Vec<_>>()
        .join("");
    
    let a_concat = (0..segments_to_keep.len())
        .map(|i| format!("[a{}]", i))
        .collect::<Vec<_>>()
        .join("");
    
    filter_parts.push(format!(
        "{}concat=n={}:v=1:a=1[outv][outa]",
        v_concat, segments_to_keep.len()
    ));
    filter_parts.push(format!(
        "{}concat=n={}:v=0:a=1[outa]",
        a_concat, segments_to_keep.len()
    ));
    
    let filter_complex = filter_parts.join(";");
    
    let status = Command::new("ffmpeg")
        .args([
            "-i", input_path,
            "-filter_complex", &filter_complex,
            "-map", "[outv]",
            "-map", "[outa]",
            "-c:v", "libx264",
            "-preset", "medium",
            "-crf", "23",
            "-c:a", "aac",
            "-b:a", "128k",
            "-y",
            output_path
        ])
        .status()
        .context("Failed to run ffmpeg for video rendering")?;
    
    if !status.success() {
        anyhow::bail!("FFmpeg video rendering failed");
    }
    
    Ok(())
}

pub fn get_file_size(path: &str) -> Result<u64> {
    let metadata = std::fs::metadata(path)
        .context("Failed to get file metadata")?;
    Ok(metadata.len())
}
