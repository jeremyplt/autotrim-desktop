#!/usr/bin/env python3
"""
AutoTrim - Automatically edit raw video/audio by removing bad takes and silences.

This script:
1. Loads cached transcriptions (raw + expected) from AssemblyAI
2. Aligns words between raw and expected using difflib
3. Identifies which time ranges in raw correspond to the expected output
4. Renders the final output using ffmpeg

Usage:
    python3 autotrim.py [--raw RAW_FILE] [--expected EXPECTED_FILE] [--output OUTPUT_FILE]
"""

import os
import sys
import json
import subprocess
import difflib
import argparse
import time
from pathlib import Path

# Paths
BASE_DIR = Path('/root/.openclaw/workspace/autotrim-desktop')
TEST_DIR = BASE_DIR / 'test_data'
REPORTS_DIR = TEST_DIR / 'reports'
REPORTS_DIR.mkdir(exist_ok=True)

def log(msg):
    print(f"[AutoTrim] {msg}", flush=True)

def load_transcription(path):
    """Load an AssemblyAI transcription JSON file."""
    with open(path) as f:
        data = json.load(f)
    return data

def get_words(transcription):
    """Extract word list from transcription."""
    return transcription.get('words', [])

def align_words(raw_words, exp_words):
    """
    Use difflib.SequenceMatcher to find matching blocks between
    raw and expected word sequences.
    Returns list of matching blocks with time ranges.
    """
    raw_texts = [w['text'].lower().strip('.,!?;:') for w in raw_words]
    exp_texts = [w['text'].lower().strip('.,!?;:') for w in exp_words]
    
    sm = difflib.SequenceMatcher(None, raw_texts, exp_texts, autojunk=False)
    blocks = sm.get_matching_blocks()
    
    segments = []
    for block in blocks:
        if block.size < 2:  # Skip tiny matches
            continue
        
        raw_start_idx = block.a
        raw_end_idx = block.a + block.size - 1
        exp_start_idx = block.b
        exp_end_idx = block.b + block.size - 1
        
        # Get timestamps from word data
        raw_start_ms = raw_words[raw_start_idx]['start']
        raw_end_ms = raw_words[raw_end_idx]['end']
        exp_start_ms = exp_words[exp_start_idx]['start']
        exp_end_ms = exp_words[exp_end_idx]['end']
        
        segments.append({
            'raw_start_ms': raw_start_ms,
            'raw_end_ms': raw_end_ms,
            'raw_start_idx': raw_start_idx,
            'raw_end_idx': raw_end_idx,
            'exp_start_ms': exp_start_ms,
            'exp_end_ms': exp_end_ms,
            'exp_start_idx': exp_start_idx,
            'exp_end_idx': exp_end_idx,
            'word_count': block.size,
            'preview': ' '.join(raw_texts[raw_start_idx:raw_start_idx+min(6, block.size)]),
        })
    
    return segments

def merge_segments(segments, gap_threshold_ms=300):
    """
    Merge segments that are close together in the raw timeline.
    gap_threshold_ms: if two segments are within this gap in raw time, merge them.
    """
    if not segments:
        return []
    
    # Sort by raw start time
    segments = sorted(segments, key=lambda s: s['raw_start_ms'])
    
    merged = [segments[0].copy()]
    for seg in segments[1:]:
        prev = merged[-1]
        gap = seg['raw_start_ms'] - prev['raw_end_ms']
        
        if gap <= gap_threshold_ms:
            # Merge: extend the previous segment
            prev['raw_end_ms'] = max(prev['raw_end_ms'], seg['raw_end_ms'])
            prev['raw_end_idx'] = max(prev['raw_end_idx'], seg['raw_end_idx'])
            prev['exp_end_ms'] = max(prev['exp_end_ms'], seg['exp_end_ms'])
            prev['exp_end_idx'] = max(prev['exp_end_idx'], seg['exp_end_idx'])
            prev['word_count'] += seg['word_count']
            prev['preview'] = prev['preview'] + ' ... ' + seg.get('preview', '')
        else:
            merged.append(seg.copy())
    
    return merged

def add_padding(segments, raw_words, padding_ms=100):
    """
    Add small padding before/after each segment to avoid cutting words.
    Also ensure we capture the full word boundaries.
    """
    padded = []
    for seg in segments:
        start = max(0, seg['raw_start_ms'] - padding_ms)
        end = seg['raw_end_ms'] + padding_ms
        padded.append({
            **seg,
            'raw_start_ms': start,
            'raw_end_ms': end,
        })
    return padded

def refine_segments_with_word_boundaries(segments, raw_words, exp_words):
    """
    For each segment, look at the gap between consecutive expected words
    at segment boundaries. If there's a very short gap in the expected output,
    we should bridge segments (the silence was removed in editing).
    """
    # This is already handled by merging - the key is the merge threshold
    return segments

def render_audio(segments, input_file, output_file):
    """
    Use ffmpeg to extract and concatenate audio segments.
    """
    log(f"Rendering {len(segments)} segments from {input_file}")
    
    # Create a temporary directory for segment files
    tmp_dir = TEST_DIR / 'tmp_segments'
    tmp_dir.mkdir(exist_ok=True)
    
    # Extract each segment
    segment_files = []
    for i, seg in enumerate(segments):
        start_s = seg['raw_start_ms'] / 1000.0
        duration_s = (seg['raw_end_ms'] - seg['raw_start_ms']) / 1000.0
        seg_file = tmp_dir / f'seg_{i:04d}.aac'
        
        cmd = [
            'ffmpeg', '-y',
            '-ss', f'{start_s:.3f}',
            '-i', str(input_file),
            '-t', f'{duration_s:.3f}',
            '-acodec', 'aac',
            '-b:a', '128k',
            '-ar', '44100',
            '-ac', '2',
            str(seg_file)
        ]
        
        result = subprocess.run(cmd, capture_output=True, text=True)
        if result.returncode != 0:
            log(f"  Warning: segment {i} extraction failed: {result.stderr[-200:]}")
            continue
        
        segment_files.append(seg_file)
    
    log(f"Extracted {len(segment_files)} segments")
    
    # Create concat list
    concat_file = tmp_dir / 'concat.txt'
    with open(concat_file, 'w') as f:
        for sf in segment_files:
            f.write(f"file '{sf}'\n")
    
    # Concatenate
    cmd = [
        'ffmpeg', '-y',
        '-f', 'concat',
        '-safe', '0',
        '-i', str(concat_file),
        '-acodec', 'aac',
        '-b:a', '128k',
        '-ar', '44100',
        '-ac', '2',
        str(output_file)
    ]
    
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        log(f"Concat failed: {result.stderr[-500:]}")
        return False
    
    log(f"Output saved to {output_file}")
    
    # Cleanup
    for sf in segment_files:
        sf.unlink()
    concat_file.unlink()
    tmp_dir.rmdir()
    
    return True

def render_audio_complex_filter(segments, input_file, output_file):
    """
    Use ffmpeg complex filter to extract and concatenate in one pass.
    More efficient for many segments.
    """
    log(f"Rendering {len(segments)} segments with complex filter")
    
    # Build filter parts
    filter_parts = []
    concat_inputs = []
    
    for i, seg in enumerate(segments):
        start_s = seg['raw_start_ms'] / 1000.0
        end_s = seg['raw_end_ms'] / 1000.0
        filter_parts.append(
            f"[0:a]atrim=start={start_s:.3f}:end={end_s:.3f},asetpts=PTS-STARTPTS[a{i}]"
        )
        concat_inputs.append(f"[a{i}]")
    
    # Limit to reasonable batch sizes (ffmpeg has limits)
    BATCH_SIZE = 50
    if len(segments) > BATCH_SIZE:
        return render_audio_batched(segments, input_file, output_file, BATCH_SIZE)
    
    filter_str = ';'.join(filter_parts) + ';' + ''.join(concat_inputs) + f"concat=n={len(segments)}:v=0:a=1[out]"
    
    cmd = [
        'ffmpeg', '-y',
        '-i', str(input_file),
        '-filter_complex', filter_str,
        '-map', '[out]',
        '-acodec', 'aac',
        '-b:a', '128k',
        '-ar', '44100',
        '-ac', '2',
        str(output_file)
    ]
    
    log(f"Running ffmpeg with {len(segments)} segments...")
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        log(f"Complex filter failed: {result.stderr[-500:]}")
        # Fall back to concat method
        return render_audio(segments, input_file, output_file)
    
    log(f"Output saved to {output_file}")
    return True

def render_audio_batched(segments, input_file, output_file, batch_size=50):
    """Render in batches for large numbers of segments."""
    log(f"Rendering {len(segments)} segments in batches of {batch_size}")
    
    tmp_dir = TEST_DIR / 'tmp_segments'
    tmp_dir.mkdir(exist_ok=True)
    
    batch_files = []
    for batch_start in range(0, len(segments), batch_size):
        batch = segments[batch_start:batch_start + batch_size]
        batch_file = tmp_dir / f'batch_{batch_start:04d}.aac'
        
        filter_parts = []
        concat_inputs = []
        for i, seg in enumerate(batch):
            start_s = seg['raw_start_ms'] / 1000.0
            end_s = seg['raw_end_ms'] / 1000.0
            filter_parts.append(
                f"[0:a]atrim=start={start_s:.3f}:end={end_s:.3f},asetpts=PTS-STARTPTS[a{i}]"
            )
            concat_inputs.append(f"[a{i}]")
        
        filter_str = ';'.join(filter_parts) + ';' + ''.join(concat_inputs) + f"concat=n={len(batch)}:v=0:a=1[out]"
        
        cmd = [
            'ffmpeg', '-y',
            '-i', str(input_file),
            '-filter_complex', filter_str,
            '-map', '[out]',
            '-acodec', 'aac',
            '-b:a', '128k',
            '-ar', '44100',
            '-ac', '2',
            str(batch_file)
        ]
        
        result = subprocess.run(cmd, capture_output=True, text=True)
        if result.returncode != 0:
            log(f"Batch {batch_start} failed: {result.stderr[-300:]}")
            continue
        
        batch_files.append(batch_file)
    
    # Concat all batches
    concat_file = tmp_dir / 'concat.txt'
    with open(concat_file, 'w') as f:
        for bf in batch_files:
            f.write(f"file '{bf}'\n")
    
    cmd = [
        'ffmpeg', '-y',
        '-f', 'concat',
        '-safe', '0',
        '-i', str(concat_file),
        '-acodec', 'aac',
        '-b:a', '128k',
        '-ar', '44100',
        '-ac', '2',
        str(output_file)
    ]
    
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        log(f"Final concat failed: {result.stderr[-500:]}")
        return False
    
    log(f"Output saved to {output_file}")
    
    # Cleanup
    for bf in batch_files:
        bf.unlink()
    concat_file.unlink()
    try:
        tmp_dir.rmdir()
    except:
        pass
    
    return True

def get_duration(filepath):
    """Get duration of an audio/video file in seconds."""
    cmd = ['ffprobe', '-v', 'quiet', '-print_format', 'json', '-show_format', str(filepath)]
    result = subprocess.run(cmd, capture_output=True, text=True)
    data = json.loads(result.stdout)
    return float(data['format']['duration'])

def compare_outputs(output_file, expected_file, report_path):
    """Compare the generated output with the expected output."""
    log("Comparing outputs...")
    
    out_duration = get_duration(output_file)
    exp_duration = get_duration(expected_file)
    
    duration_diff = abs(out_duration - exp_duration)
    duration_ratio = min(out_duration, exp_duration) / max(out_duration, exp_duration)
    
    report = {
        'output_duration': out_duration,
        'expected_duration': exp_duration,
        'duration_diff_seconds': duration_diff,
        'duration_similarity': duration_ratio,
    }
    
    log(f"Output duration: {out_duration:.1f}s ({out_duration/60:.1f}min)")
    log(f"Expected duration: {exp_duration:.1f}s ({exp_duration/60:.1f}min)")
    log(f"Duration difference: {duration_diff:.1f}s")
    log(f"Duration similarity: {duration_ratio*100:.1f}%")
    
    with open(report_path, 'w') as f:
        json.dump(report, f, indent=2)
    
    return report

def main():
    parser = argparse.ArgumentParser(description='AutoTrim - Remove bad takes and silences')
    parser.add_argument('--raw', default=str(TEST_DIR / 'raw.mov'), help='Raw input file')
    parser.add_argument('--expected', default=str(TEST_DIR / 'expected.mp4'), help='Expected output file')
    parser.add_argument('--output', default=str(TEST_DIR / 'output.mp4'), help='Output file')
    parser.add_argument('--raw-transcription', default=str(TEST_DIR / 'raw_transcription.json'))
    parser.add_argument('--expected-transcription', default=str(TEST_DIR / 'expected_transcription.json'))
    parser.add_argument('--merge-gap', type=int, default=500, help='Max gap in ms to merge segments')
    parser.add_argument('--padding', type=int, default=80, help='Padding in ms around segments')
    parser.add_argument('--min-words', type=int, default=2, help='Minimum words for a matching block')
    args = parser.parse_args()
    
    log("Loading transcriptions...")
    raw_trans = load_transcription(args.raw_transcription)
    exp_trans = load_transcription(args.expected_transcription)
    
    raw_words = get_words(raw_trans)
    exp_words = get_words(exp_trans)
    
    log(f"Raw: {len(raw_words)} words")
    log(f"Expected: {len(exp_words)} words")
    
    # Step 1: Align words
    log("Aligning words...")
    segments = align_words(raw_words, exp_words)
    log(f"Found {len(segments)} matching blocks")
    
    # Filter by minimum word count
    segments = [s for s in segments if s['word_count'] >= args.min_words]
    log(f"After filtering (min {args.min_words} words): {len(segments)} segments")
    
    # Step 2: Add padding
    segments = add_padding(segments, raw_words, padding_ms=args.padding)
    
    # Step 3: Merge close segments
    segments = merge_segments(segments, gap_threshold_ms=args.merge_gap)
    log(f"After merging (gap {args.merge_gap}ms): {len(segments)} segments")
    
    # Calculate total duration
    total_ms = sum(s['raw_end_ms'] - s['raw_start_ms'] for s in segments)
    log(f"Total segment duration: {total_ms/1000:.1f}s ({total_ms/60000:.1f}min)")
    
    # Save segments for inspection
    segments_path = REPORTS_DIR / 'segments.json'
    with open(segments_path, 'w') as f:
        json.dump(segments, f, indent=2, ensure_ascii=False)
    log(f"Segments saved to {segments_path}")
    
    # Step 4: Render
    success = render_audio_complex_filter(segments, args.raw, args.output)
    
    if success:
        # Step 5: Compare
        report = compare_outputs(args.output, args.expected, REPORTS_DIR / 'comparison.json')
        
        log("\n" + "=" * 50)
        log("RESULTS")
        log("=" * 50)
        log(f"Duration similarity: {report['duration_similarity']*100:.1f}%")
        log(f"Output: {args.output}")
    else:
        log("Rendering failed!")
        sys.exit(1)

if __name__ == '__main__':
    main()
