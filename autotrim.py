#!/usr/bin/env python3
"""
AutoTrim - Automatically edit raw video/audio by removing bad takes and silences.

This script:
1. Loads cached transcriptions (raw + expected) from AssemblyAI
2. Aligns words between raw and expected using difflib
3. Identifies which time ranges in raw correspond to the expected output
4. Splits matched blocks at large internal gaps (removes silences/bad takes)
5. Renders the final output using ffmpeg

Usage:
    python3 autotrim.py [--raw RAW_FILE] [--expected EXPECTED_FILE] [--output OUTPUT_FILE]
"""

import os
import sys
import json
import subprocess
import difflib
import argparse
import re
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

def normalize_word(text):
    """Normalize a word for matching: lowercase, strip all punctuation INCLUDING hyphens."""
    # Remove ALL punctuation including hyphens to match "text-to-speech" with "text to speech"
    return re.sub(r'[^a-z0-9àâäéèêëïîôùûüÿçœæ]', '', text.lower())

def align_words(raw_words, exp_words, min_block_size=2, max_internal_gap_ms=1500):
    """
    Use difflib.SequenceMatcher to find matching blocks between
    raw and expected word sequences, then split blocks at large internal gaps.
    
    v3: Reduced min_block_size to 1 for segments near larger blocks to capture small gaps.
    
    Returns list of segments with time ranges.
    """
    raw_texts = [normalize_word(w['text']) for w in raw_words]
    exp_texts = [normalize_word(w['text']) for w in exp_words]
    
    sm = difflib.SequenceMatcher(None, raw_texts, exp_texts, autojunk=False)
    blocks = sm.get_matching_blocks()
    
    log(f"  difflib found {len(blocks)} raw matching blocks")
    
    segments = []
    for block_idx, block in enumerate(blocks):
        # Allow size=1 if this block is close to a larger block (within 10s in raw timeline)
        is_near_large_block = False
        if block.size == 1 and block.a < len(raw_words):
            for other_block in blocks:
                if other_block.size >= 5 and other_block != block:
                    time_dist = abs(raw_words[block.a]['start'] - raw_words[other_block.a]['start'])
                    if time_dist < 15000:  # Within 15s
                        is_near_large_block = True
                        break
        
        effective_min_size = 1 if is_near_large_block else min_block_size
        
        if block.size < effective_min_size:
            continue
        
        # Split this block at large internal gaps in the raw audio
        sub_start = block.a
        sub_exp_start = block.b
        
        for i in range(block.size - 1):
            raw_idx = block.a + i
            next_raw_idx = block.a + i + 1
            gap = raw_words[next_raw_idx]['start'] - raw_words[raw_idx]['end']
            
            if gap > max_internal_gap_ms:
                # End current sub-block here
                sub_size = (raw_idx + 1) - sub_start
                if sub_size >= effective_min_size:
                    segments.append(_make_segment(
                        raw_words, exp_words, raw_texts,
                        sub_start, sub_size,
                        sub_exp_start, sub_size
                    ))
                # Start new sub-block
                sub_start = next_raw_idx
                sub_exp_start = block.b + i + 1
        
        # Final sub-block
        sub_size = (block.a + block.size) - sub_start
        if sub_size >= effective_min_size:
            segments.append(_make_segment(
                raw_words, exp_words, raw_texts,
                sub_start, sub_size,
                sub_exp_start, sub_size
            ))
    
    return segments

def _make_segment(raw_words, exp_words, raw_texts, raw_start_idx, size, exp_start_idx, exp_size):
    """Create a segment dict from indices."""
    raw_end_idx = raw_start_idx + size - 1
    exp_end_idx = exp_start_idx + exp_size - 1
    
    return {
        'raw_start_ms': raw_words[raw_start_idx]['start'],
        'raw_end_ms': raw_words[raw_end_idx]['end'],
        'raw_start_idx': raw_start_idx,
        'raw_end_idx': raw_end_idx,
        'exp_start_ms': exp_words[exp_start_idx]['start'],
        'exp_end_ms': exp_words[exp_end_idx]['end'],
        'exp_start_idx': exp_start_idx,
        'exp_end_idx': exp_end_idx,
        'word_count': size,
        'preview': ' '.join(raw_texts[raw_start_idx:raw_start_idx + min(6, size)]),
    }

def fill_gaps_with_fuzzy_matching(segments, raw_words, exp_words, max_gap_words=15, max_gap_time_ms=5000):
    """
    After initial alignment, look for small gaps in the expected timeline and try to find
    matching content in the raw timeline using fuzzy substring matching.
    
    This helps recover content that difflib missed due to transcription differences
    (e.g., "text-to-speech" vs "text to speech" or "youtubeclotestbot" vs "YouTube CLO Test Bot").
    """
    if len(segments) < 2:
        return segments
    
    segments = sorted(segments, key=lambda s: s['exp_start_ms'])
    new_segments = []
    filled_gaps = 0
    
    for i in range(len(segments) - 1):
        current = segments[i]
        next_seg = segments[i + 1]
        new_segments.append(current)
        
        # Check for gap in expected timeline
        exp_gap_start_idx = current['exp_end_idx'] + 1
        exp_gap_end_idx = next_seg['exp_start_idx'] - 1
        
        if exp_gap_end_idx < exp_gap_start_idx:
            continue  # No gap
        
        gap_word_count = exp_gap_end_idx - exp_gap_start_idx + 1
        exp_gap_time = next_seg['exp_start_ms'] - current['exp_end_ms']
        
        # Only try to fill small gaps
        if gap_word_count > max_gap_words or exp_gap_time > max_gap_time_ms:
            continue
        
        # Get the expected text in the gap
        exp_gap_text = ' '.join([exp_words[j]['text'] for j in range(exp_gap_start_idx, exp_gap_end_idx + 1)])
        exp_gap_normalized = ''.join([normalize_word(exp_words[j]['text']) for j in range(exp_gap_start_idx, exp_gap_end_idx + 1)])
        
        # Look for this content in the raw timeline between current and next segment
        raw_search_start_idx = current['raw_end_idx'] + 1
        raw_search_end_idx = next_seg['raw_start_idx'] - 1
        
        if raw_search_end_idx < raw_search_start_idx:
            continue
        
        # Try sliding window matching with fuzzy comparison
        best_match_idx = None
        best_match_score = 0
        
        for window_size in [gap_word_count, gap_word_count - 1, gap_word_count + 1]:
            if window_size < 1:
                continue
            for start_idx in range(raw_search_start_idx, min(raw_search_end_idx - window_size + 2, raw_search_start_idx + 50)):
                end_idx = start_idx + window_size - 1
                if end_idx > raw_search_end_idx:
                    break
                
                raw_window_normalized = ''.join([normalize_word(raw_words[j]['text']) for j in range(start_idx, end_idx + 1)])
                
                # Compare normalized strings
                if raw_window_normalized == exp_gap_normalized:
                    score = 1.0  # Perfect match
                else:
                    # Fuzzy match: check substring containment
                    if exp_gap_normalized in raw_window_normalized or raw_window_normalized in exp_gap_normalized:
                        score = min(len(exp_gap_normalized), len(raw_window_normalized)) / max(len(exp_gap_normalized), len(raw_window_normalized))
                    else:
                        # Character-level similarity
                        matches = sum(1 for a, b in zip(exp_gap_normalized, raw_window_normalized) if a == b)
                        score = matches / max(len(exp_gap_normalized), len(raw_window_normalized))
                
                if score > best_match_score and score >= 0.6:  # Threshold for acceptance
                    best_match_score = score
                    best_match_idx = start_idx
                    best_match_end_idx = end_idx
        
        # If found a good match, add it as a segment
        if best_match_idx is not None:
            new_seg = {
                'raw_start_ms': raw_words[best_match_idx]['start'],
                'raw_end_ms': raw_words[best_match_end_idx]['end'],
                'raw_start_idx': best_match_idx,
                'raw_end_idx': best_match_end_idx,
                'exp_start_ms': exp_words[exp_gap_start_idx]['start'],
                'exp_end_ms': exp_words[exp_gap_end_idx]['end'],
                'exp_start_idx': exp_gap_start_idx,
                'exp_end_idx': exp_gap_end_idx,
                'word_count': gap_word_count,
                'preview': f"[FILLED GAP {best_match_score:.0%}] {exp_gap_text[:50]}",
            }
            new_segments.append(new_seg)
            filled_gaps += 1
            log(f"  Filled gap: exp {exp_gap_start_idx}-{exp_gap_end_idx} ({gap_word_count}w) matched to raw {best_match_idx}-{best_match_end_idx} (score={best_match_score:.0%}): {exp_gap_text[:60]}")
    
    # Don't forget the last segment
    if segments:
        new_segments.append(segments[-1])
    
    if filled_gaps > 0:
        log(f"  Filled {filled_gaps} gaps with fuzzy matching")
        # Re-sort by expected timeline
        new_segments = sorted(new_segments, key=lambda s: s['exp_start_ms'])
    
    return new_segments

def detect_and_remove_retakes(segments, raw_words, base_threshold=0.55):
    """
    Enhanced retake detection with multi-segment lookahead (v2).
    
    Improvements:
    - Looks ahead 2-3 segments (not just adjacent)
    - Adaptive threshold based on segment length
    - Checks first 3-4 words specifically (common retake pattern)
    - Time window: segments within 15s are compared more aggressively
    """
    if not segments:
        return []
    
    segments = sorted(segments, key=lambda s: s['raw_start_ms'])
    to_remove = set()
    
    for i in range(len(segments)):
        if i in to_remove:
            continue
            
        seg1 = segments[i]
        
        # Only check short segments as potential retakes
        if seg1['word_count'] >= 20:
            continue
        
        # Adaptive threshold: shorter segments get lower threshold
        if seg1['word_count'] <= 6:
            threshold = 0.50
        elif seg1['word_count'] <= 10:
            threshold = 0.55
        else:
            threshold = 0.65
        
        # Look ahead up to 3 segments or 15 seconds
        for j in range(i + 1, min(i + 4, len(segments))):
            if j in to_remove:
                continue
                
            seg2 = segments[j]
            
            # Time window check
            time_gap = (seg2['raw_start_ms'] - seg1['raw_end_ms']) / 1000.0
            if time_gap > 15.0:
                break
            
            # Get words
            words1_all = [raw_words[k]['text'] for k in range(seg1['raw_start_idx'], seg1['raw_end_idx'] + 1)]
            words2_all = [raw_words[k]['text'] for k in range(seg2['raw_start_idx'], seg2['raw_end_idx'] + 1)]
            
            # Check first-word pattern (common retake signature)
            first_words1 = words1_all[:min(4, len(words1_all))]
            first_words2 = words2_all[:min(4, len(words2_all))]
            
            first_match = sum(1 for w1, w2 in zip(first_words1, first_words2) 
                            if normalize_word(w1) == normalize_word(w2))
            first_match_ratio = first_match / len(first_words1) if first_words1 else 0
            
            # Normalize words for content comparison
            words1 = set(normalize_word(w) for w in words1_all)
            words2 = set(normalize_word(w) for w in words2_all)
            
            words1 = {w for w in words1 if w}
            words2 = {w for w in words2 if w}
            
            if not words1 or not words2:
                continue
            
            # Calculate overlap
            overlap = len(words1 & words2)
            similarity = overlap / len(words1)
            
            # Decision logic
            is_retake = False
            reason = ""
            
            # Case 1: High first-word match + decent overall overlap
            if first_match_ratio >= 0.75 and similarity >= 0.40 and seg2['word_count'] >= seg1['word_count']:
                is_retake = True
                reason = f"first-word {first_match_ratio*100:.0f}%, content {similarity*100:.0f}%"
            
            # Case 2: Very high overall similarity
            elif similarity >= threshold and seg2['word_count'] > seg1['word_count']:
                is_retake = True
                reason = f"high-sim {similarity*100:.0f}%"
            
            # Case 3: Within short time window + good overlap
            elif time_gap < 5.0 and similarity >= threshold * 0.85 and seg2['word_count'] >= seg1['word_count']:
                is_retake = True
                reason = f"close-time {time_gap:.1f}s, sim {similarity*100:.0f}%"
            
            if is_retake:
                to_remove.add(i)
                log(f"  Retake: seg{i} ({seg1['word_count']}w) → seg{j} ({seg2['word_count']}w) [{reason}]")
                break  # Found a better version, stop looking
    
    result = [seg for i, seg in enumerate(segments) if i not in to_remove]
    log(f"  Removed {len(to_remove)} retakes, {len(result)} segments remain")
    return result

def merge_segments(segments, gap_threshold_ms=500):
    """
    Merge segments that are close together in the raw timeline.
    """
    if not segments:
        return []
    
    segments = sorted(segments, key=lambda s: s['raw_start_ms'])
    
    merged = [segments[0].copy()]
    for seg in segments[1:]:
        prev = merged[-1]
        gap = seg['raw_start_ms'] - prev['raw_end_ms']
        
        if gap <= gap_threshold_ms:
            prev['raw_end_ms'] = max(prev['raw_end_ms'], seg['raw_end_ms'])
            prev['raw_end_idx'] = max(prev['raw_end_idx'], seg['raw_end_idx'])
            prev['exp_end_ms'] = max(prev['exp_end_ms'], seg['exp_end_ms'])
            prev['exp_end_idx'] = max(prev['exp_end_idx'], seg['exp_end_idx'])
            prev['word_count'] += seg['word_count']
            prev['preview'] = prev.get('preview', '') + ' ... ' + seg.get('preview', '')
        else:
            merged.append(seg.copy())
    
    return merged

def add_padding(segments, padding_ms=100):
    """Add small padding before/after each segment."""
    padded = []
    for seg in segments:
        padded.append({
            **seg,
            'raw_start_ms': max(0, seg['raw_start_ms'] - padding_ms),
            'raw_end_ms': seg['raw_end_ms'] + padding_ms,
        })
    return padded

def remove_overlaps(segments):
    """Ensure no segments overlap in the raw timeline."""
    if not segments:
        return []
    
    segments = sorted(segments, key=lambda s: s['raw_start_ms'])
    result = [segments[0].copy()]
    
    for seg in segments[1:]:
        prev = result[-1]
        if seg['raw_start_ms'] < prev['raw_end_ms']:
            # Overlap: trim the start of current segment
            seg = seg.copy()
            seg['raw_start_ms'] = prev['raw_end_ms']
        if seg['raw_start_ms'] < seg['raw_end_ms']:
            result.append(seg)
    
    return result

def remove_micro_segments(segments, min_duration_ms=1500, min_words=5):
    """
    Remove very short segments that are likely fragments or noise.
    Only remove if the segment is isolated (not close to others).
    v2: More lenient - keep filled gaps and segments near larger ones.
    """
    if not segments:
        return []
    
    segments = sorted(segments, key=lambda s: s['raw_start_ms'])
    result = []
    
    for i, seg in enumerate(segments):
        duration = seg['raw_end_ms'] - seg['raw_start_ms']
        
        # Always keep filled gaps (they're legit content we recovered)
        if 'FILLED GAP' in seg.get('preview', ''):
            result.append(seg)
            continue
        
        # Keep if duration and word count are above thresholds
        if duration >= min_duration_ms or seg['word_count'] >= min_words:
            result.append(seg)
            continue
        
        # Check if isolated (far from neighbors)
        prev_gap = (seg['raw_start_ms'] - segments[i-1]['raw_end_ms']) if i > 0 else 999999
        next_gap = (segments[i+1]['raw_start_ms'] - seg['raw_end_ms']) if i < len(segments)-1 else 999999
        
        # Keep if close to neighbors (< 5s gap) - even more lenient now
        if prev_gap < 5000 or next_gap < 5000:
            result.append(seg)
        else:
            log(f"  Removing micro-segment: {duration/1000:.1f}s, {seg['word_count']}w")
    
    return result

def render_with_concat(segments, input_file, output_file):
    """
    Render by extracting each segment individually, then concatenating.
    Uses -ss before -i for fast seeking on large files.
    """
    log(f"Rendering {len(segments)} segments via concat method")
    
    tmp_dir = TEST_DIR / 'tmp_segments'
    tmp_dir.mkdir(exist_ok=True)
    
    # Clean any leftover temp files
    for f in tmp_dir.glob('seg_*.ts'):
        f.unlink()
    
    segment_files = []
    failed = 0
    
    for i, seg in enumerate(segments):
        start_s = seg['raw_start_ms'] / 1000.0
        duration_s = (seg['raw_end_ms'] - seg['raw_start_ms']) / 1000.0
        
        if duration_s <= 0:
            continue
        
        # Use .ts (MPEG-TS) for seamless concatenation
        seg_file = tmp_dir / f'seg_{i:04d}.ts'
        
        cmd = [
            'ffmpeg', '-y',
            '-ss', f'{start_s:.3f}',
            '-i', str(input_file),
            '-t', f'{duration_s:.3f}',
            '-c:a', 'aac',
            '-b:a', '128k',
            '-ar', '44100',
            '-ac', '2',
            '-vn',  # no video
            '-f', 'mpegts',
            str(seg_file)
        ]
        
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
        if result.returncode != 0:
            log(f"  Warning: segment {i} failed (start={start_s:.1f}s, dur={duration_s:.1f}s)")
            log(f"    Error: {result.stderr[-200:]}")
            failed += 1
            continue
        
        # Verify file exists and is not empty
        if seg_file.exists() and seg_file.stat().st_size > 0:
            segment_files.append(seg_file)
        else:
            log(f"  Warning: segment {i} produced empty or missing file")
            if seg_file.exists():
                log(f"    File exists but size is {seg_file.stat().st_size}")
            failed += 1
    
    if failed > 0:
        log(f"  {failed} segments failed, {len(segment_files)} succeeded")
    
    if not segment_files:
        log("ERROR: No segments extracted!")
        return False
    
    # Concatenate using concat protocol
    concat_file = tmp_dir / 'concat.txt'
    with open(concat_file, 'w') as f:
        for sf in segment_files:
            f.write(f"file '{sf}'\n")
    
    cmd = [
        'ffmpeg', '-y',
        '-f', 'concat',
        '-safe', '0',
        '-i', str(concat_file),
        '-c:a', 'aac',
        '-b:a', '128k',
        '-ar', '44100',
        '-ac', '2',
        '-vn',
        str(output_file)
    ]
    
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
    if result.returncode != 0:
        log(f"Concat failed: {result.stderr[-500:]}")
        return False
    
    log(f"Output saved to {output_file}")
    
    # Cleanup
    for sf in segment_files:
        try:
            sf.unlink()
        except:
            pass
    try:
        concat_file.unlink()
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
    parser.add_argument('--merge-gap', type=int, default=2500, help='Max gap in ms to merge segments')
    parser.add_argument('--padding', type=int, default=150, help='Padding in ms around segments')
    parser.add_argument('--min-words', type=int, default=2, help='Minimum words for a matching block')
    parser.add_argument('--max-internal-gap', type=int, default=1500, help='Max gap within a block before splitting (ms)')
    parser.add_argument('--dry-run', action='store_true', help='Only compute segments, skip rendering')
    args = parser.parse_args()
    
    log("Loading transcriptions...")
    raw_trans = load_transcription(args.raw_transcription)
    exp_trans = load_transcription(args.expected_transcription)
    
    raw_words = get_words(raw_trans)
    exp_words = get_words(exp_trans)
    
    log(f"Raw: {len(raw_words)} words, {raw_words[-1]['end']/1000:.1f}s")
    log(f"Expected: {len(exp_words)} words, {exp_words[-1]['end']/1000:.1f}s")
    
    # Step 1: Align words with gap splitting
    log(f"Aligning words (min_words={args.min_words}, max_internal_gap={args.max_internal_gap}ms)...")
    segments = align_words(
        raw_words, exp_words,
        min_block_size=args.min_words,
        max_internal_gap_ms=args.max_internal_gap
    )
    log(f"Found {len(segments)} segments after gap splitting")
    
    # Step 1.5: Fill gaps with fuzzy matching
    log(f"Filling gaps with fuzzy matching...")
    segments = fill_gaps_with_fuzzy_matching(segments, raw_words, exp_words, max_gap_words=15, max_gap_time_ms=8000)
    log(f"After gap filling: {len(segments)} segments")
    
    # Step 2: Detect and remove retakes (BEFORE merging)
    log(f"Detecting retakes...")
    segments = detect_and_remove_retakes(segments, raw_words, base_threshold=0.55)
    
    # Step 3: Add padding
    segments = add_padding(segments, padding_ms=args.padding)
    
    # Step 4: Merge close segments
    segments = merge_segments(segments, gap_threshold_ms=args.merge_gap)
    log(f"After merging (gap {args.merge_gap}ms): {len(segments)} segments")
    
    # Step 5: Remove micro-segments
    log(f"Removing micro-segments...")
    segments = remove_micro_segments(segments, min_duration_ms=2000, min_words=8)
    log(f"After micro-segment removal: {len(segments)} segments")
    
    # Step 6: Remove overlaps
    segments = remove_overlaps(segments)
    
    # Calculate stats
    total_ms = sum(s['raw_end_ms'] - s['raw_start_ms'] for s in segments)
    log(f"Total segment duration: {total_ms/1000:.1f}s ({total_ms/60000:.1f}min)")
    log(f"Target (expected): {exp_words[-1]['end']/1000:.1f}s")
    log(f"Ratio: {total_ms / exp_words[-1]['end'] * 100:.1f}%")
    
    # Save segments for inspection
    segments_path = REPORTS_DIR / 'segments.json'
    with open(segments_path, 'w') as f:
        json.dump(segments, f, indent=2, ensure_ascii=False)
    log(f"Segments saved to {segments_path}")
    
    if args.dry_run:
        log("Dry run - skipping rendering")
        return
    
    # Step 5: Render
    log("Starting render...")
    t0 = time.time()
    success = render_with_concat(segments, args.raw, args.output)
    render_time = time.time() - t0
    log(f"Render took {render_time:.1f}s")
    
    if success:
        # Step 6: Compare
        report = compare_outputs(args.output, args.expected, REPORTS_DIR / 'comparison.json')
        
        log("\n" + "=" * 50)
        log("RESULTS")
        log("=" * 50)
        log(f"Duration similarity: {report['duration_similarity']*100:.1f}%")
        log(f"Output: {args.output}")
        
        if report['duration_similarity'] >= 0.95:
            log("✅ SUCCESS: Duration within 5% of expected!")
        else:
            log(f"❌ Need {0.95*100:.0f}% similarity, got {report['duration_similarity']*100:.1f}%")
    else:
        log("Rendering failed!")
        sys.exit(1)

if __name__ == '__main__':
    main()
