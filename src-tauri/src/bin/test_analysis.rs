//! Quick test binary: re-runs ONLY the AI analysis on existing transcription data.
//! Usage: cargo run --bin test_analysis

use std::collections::HashSet;

// Import from the library crate
use autotrim_desktop_lib::transcription::{self, Transcription};

#[tokio::main]
async fn main() {
    let transcription_path = "/Users/jeremy/Downloads/claw_raw_1_transcription.json";
    let output_dir = "/Users/jeremy/Downloads";

    // Load API key
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .unwrap_or_else(|_| {
            let env_content = std::fs::read_to_string("/Users/jeremy/Projects/autotrim-desktop/.env")
                .expect("Failed to read .env");
            env_content.lines()
                .find(|l| l.starts_with("ANTHROPIC_API_KEY="))
                .map(|l| l["ANTHROPIC_API_KEY=".len()..].trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty())
                .expect("ANTHROPIC_API_KEY not found in .env")
        });

    eprintln!("Loading transcription from {}...", transcription_path);
    let transcription_json = std::fs::read_to_string(transcription_path)
        .expect("Failed to read transcription file");
    let transcription: Transcription = serde_json::from_str(&transcription_json)
        .expect("Failed to parse transcription JSON");

    eprintln!("Loaded {} words", transcription.words.len());

    // Segment into chunks (same as processor.rs)
    let chunks = transcription::segment_into_chunks(&transcription.words, 0.5);
    eprintln!("Segmented into {} chunks", chunks.len());

    // Save chunks
    let chunks_json = serde_json::to_string_pretty(&chunks).unwrap();
    std::fs::write(format!("{}/claw_raw_2_chunks_v2.json", output_dir), &chunks_json).ok();

    // Call Claude with extended thinking
    eprintln!("Calling Claude Sonnet with extended thinking (mode: moderate)...");
    let start = std::time::Instant::now();
    // Temporarily test with thinking=false to isolate the issue
    let keep_ids = transcription::determine_keep_ranges(&chunks, &api_key, "moderate")
        .await
        .expect("AI analysis failed");
    // Note: determine_keep_ranges uses thinking=true internally
    let elapsed = start.elapsed();
    eprintln!("Claude responded in {:.1}s", elapsed.as_secs_f64());

    // Save raw AI keep_ids
    let keep_json = serde_json::to_string_pretty(&keep_ids).unwrap();
    std::fs::write(format!("{}/claw_raw_3_ai_keep_ids_v2.json", output_dir), &keep_json).ok();

    eprintln!("AI kept {}/{} chunks ({} removed)", keep_ids.len(), chunks.len(), chunks.len() - keep_ids.len());

    // Generate report
    let keep_set: HashSet<usize> = keep_ids.iter().copied().collect();
    let mut report = String::new();
    let mut total_kept_duration = 0.0;
    let mut total_removed_duration = 0.0;

    for chunk in &chunks {
        let status = if keep_set.contains(&chunk.id) { "KEEP  " } else { "REMOVE" };
        let preview: String = chunk.text.chars().take(80).collect();
        let ellipsis = if chunk.text.chars().count() > 80 { "..." } else { "" };
        report.push_str(&format!("{} [{}] {:.1}s-{:.1}s ({} words): \"{}{}\"\n",
            status, chunk.id, chunk.start, chunk.end, chunk.word_count, preview, ellipsis));
        if keep_set.contains(&chunk.id) {
            total_kept_duration += chunk.end - chunk.start;
        } else {
            total_removed_duration += chunk.end - chunk.start;
        }
    }

    report.push_str(&format!("\n--- Summary ---\n"));
    report.push_str(&format!("Keep segments: {}\n", keep_ids.len()));
    report.push_str(&format!("Kept speech duration: {:.1}s ({:.1} min)\n", total_kept_duration, total_kept_duration / 60.0));
    report.push_str(&format!("Removed speech duration: {:.1}s ({:.1} min)\n", total_removed_duration, total_removed_duration / 60.0));

    std::fs::write(format!("{}/claw_raw_v2_report.txt", output_dir), &report).ok();
    eprintln!("\nReport saved to {}/claw_raw_v2_report.txt", output_dir);
    eprintln!("Kept speech: {:.1}s ({:.1} min)", total_kept_duration, total_kept_duration / 60.0);
    eprintln!("Removed speech: {:.1}s ({:.1} min)", total_removed_duration, total_removed_duration / 60.0);
}
