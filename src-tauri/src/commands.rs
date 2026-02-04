use serde::{Deserialize, Serialize};
use crate::{ffmpeg, processor};

#[derive(Debug, Serialize, Deserialize)]
pub struct VideoInfo {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub duration_seconds: f64,
}

#[tauri::command]
pub fn check_ffmpeg() -> bool {
    ffmpeg::check_ffmpeg_installed()
}

#[tauri::command]
pub fn get_video_info(path: String) -> Result<VideoInfo, String> {
    let metadata = ffmpeg::get_video_info(&path)
        .map_err(|e| format!("Failed to get video info: {}", e))?;
    
    let size_bytes = ffmpeg::get_file_size(&path)
        .map_err(|e| format!("Failed to get file size: {}", e))?;
    
    let name = std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string();
    
    Ok(VideoInfo {
        path,
        name,
        size_bytes,
        duration_seconds: metadata.duration,
    })
}

#[tauri::command]
pub fn start_processing(
    path: String,
    settings: processor::ProcessingSettings,
) -> Result<String, String> {
    // Get OpenAI API key from environment
    let api_key = get_openai_api_key()
        .ok_or_else(|| "OpenAI API key not found. Please set OPENAI_API_KEY environment variable.".to_string())?;
    
    let job_id = processor::start_processing(path, settings, api_key);
    Ok(job_id)
}

#[tauri::command]
pub fn get_progress(job_id: String) -> Result<processor::Progress, String> {
    processor::get_progress(&job_id)
        .ok_or_else(|| "Job not found".to_string())
}

#[tauri::command]
pub fn get_result(job_id: String) -> Result<processor::ProcessingResult, String> {
    processor::get_result(&job_id)
        .ok_or_else(|| "Result not found".to_string())
}

#[tauri::command]
pub fn cancel_processing(job_id: String) -> Result<(), String> {
    processor::cancel_processing(&job_id);
    Ok(())
}

#[tauri::command]
pub fn open_output_folder(path: String) -> Result<(), String> {
    let folder_path = std::path::Path::new(&path)
        .parent()
        .ok_or_else(|| "Invalid path".to_string())?;
    
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }
    
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }
    
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }
    
    Ok(())
}

fn get_openai_api_key() -> Option<String> {
    // Try to get from environment variable
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        // Remove quotes if present
        let key = key.trim().trim_matches('"').to_string();
        if !key.is_empty() {
            return Some(key);
        }
    }
    
    // Try to read from .env file in workspace
    let env_path = std::path::Path::new("/root/.openclaw/workspace/.env");
    if let Ok(contents) = std::fs::read_to_string(env_path) {
        for line in contents.lines() {
            if line.starts_with("OPENAI_API_KEY=") {
                let key = line.trim_start_matches("OPENAI_API_KEY=")
                    .trim()
                    .trim_matches('"')
                    .to_string();
                if !key.is_empty() {
                    return Some(key);
                }
            }
        }
    }
    
    None
}
