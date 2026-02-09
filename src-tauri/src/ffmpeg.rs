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
    pub frame_rate: f64,
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
            "-show_entries", "stream=width,height,duration,r_frame_rate",
            "-of", "json",
            path
        ])
        .output()
        .context("Failed to run ffprobe")?;

    let stdout = String::from_utf8_lossy(&output.stdout);

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

    // Parse frame rate (format: "60000/1001" or "30/1")
    let frame_rate = stream.get("r_frame_rate")
        .and_then(|v| v.as_str())
        .and_then(|s| {
            let parts: Vec<&str> = s.split('/').collect();
            if parts.len() == 2 {
                let num = parts[0].parse::<f64>().ok()?;
                let den = parts[1].parse::<f64>().ok()?;
                if den > 0.0 { Some(num / den) } else { None }
            } else {
                s.parse::<f64>().ok()
            }
        })
        .unwrap_or(30.0);

    Ok(VideoMetadata {
        duration,
        width,
        height,
        frame_rate,
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
    frame_rate: f64,
) -> Result<String> {
    if segments_to_keep.is_empty() {
        anyhow::bail!("No segments to keep");
    }

    eprintln!("Rendering {} segments via select/aselect single-pass (fps: {:.2})", segments_to_keep.len(), frame_rate);

    on_progress(0.0);

    // Build select expression: between(t,S0,E0)+between(t,S1,E1)+...
    let select_expr: String = segments_to_keep.iter()
        .map(|s| format!("between(t\\,{:.3}\\,{:.3})", s.start, s.end))
        .collect::<Vec<_>>()
        .join("+");

    let vf = format!(
        "select='{}',setpts=N/{:.6}/TB",
        select_expr, frame_rate
    );
    let af = format!(
        "aselect='{}',asetpts=N/SR/TB",
        select_expr
    );

    // Write filters to file to avoid command line length limits
    let vf_path = temp_dir.join("vf.txt");
    let af_path = temp_dir.join("af.txt");
    std::fs::write(&vf_path, &vf).context("Failed to write video filter")?;
    std::fs::write(&af_path, &af).context("Failed to write audio filter")?;

    on_progress(0.05);

    let output_mp4 = std::path::Path::new(output_path)
        .with_extension("mp4")
        .to_str()
        .unwrap()
        .to_string();

    // Render to temp dir first, then move to final location.
    // This avoids macOS file system interference (Spotlight, Quick Look, Finder)
    // during FFmpeg's faststart second pass which needs to re-open the output file.
    let temp_output = temp_dir.join("render_output.mp4");

    // Single-pass render with hardware encoding
    // select/aselect picks only the frames/samples in our keep ranges
    // setpts/asetpts re-timestamps them sequentially → perfect sync
    let filter_complex = format!(
        "[0:v]{}[outv];[0:a]{}[outa]",
        vf, af
    );
    let filter_path = temp_dir.join("filter.txt");
    std::fs::write(&filter_path, &filter_complex)
        .context("Failed to write filter script")?;

    let status = Command::new("ffmpeg")
        .args([
            "-i", input_path,
            "-filter_complex_script", filter_path.to_str().unwrap(),
            "-map", "[outv]",
            "-map", "[outa]",
            "-c:v", "h264_videotoolbox",
            "-b:v", "20M",
            "-c:a", "aac",
            "-b:a", "192k",
            "-movflags", "+faststart",
            "-y",
            temp_output.to_str().unwrap(),
        ])
        .status()
        .context("Failed to run ffmpeg for rendering")?;

    if !status.success() {
        anyhow::bail!("FFmpeg rendering failed");
    }

    // Move from temp to final location (avoids partial files in user-visible folders)
    std::fs::rename(&temp_output, &output_mp4)
        .or_else(|_| {
            // rename() fails across filesystems, fall back to copy + delete
            std::fs::copy(&temp_output, &output_mp4)
                .and_then(|_| std::fs::remove_file(&temp_output))
        })
        .context("Failed to move rendered video to output location")?;

    on_progress(1.0);
    Ok(output_mp4)
}

pub fn get_file_size(path: &str) -> Result<u64> {
    let metadata = std::fs::metadata(path)
        .context("Failed to get file metadata")?;
    Ok(metadata.len())
}
