# Iteration 1: AutoTrim Improvements

## Current State
- Similarity: 95.55% (target: 99%)
- Duration: 30.6 min (target: 30.1 min)  
- Ratio: 101.7% (only 1.7% over!)

## Issues Identified

### 1. Extra content (should remove):
- "me suis dit que ce n'est" at 3:28 (0.3s, 6 words)
- "OpenCode/OpenClo. Ici tu vas devoir mettre ton propre" at 13:49 in output (from raw 27:39, 2.7s, 8 words)

### 2. Missing content (should keep):
- "les cron jobs, c'est-à-dire les" - difflib alignment error (1.6s, 5 words)
- 2 phrases not found in raw (transcription mismatch - NOT FIXABLE)

## Root Causes

### Issue 1: Retakes Not Detected
The 2 extra passages are retakes where the speaker repeated/corrected himself:
- "me suis dit que ce n'est" - probably a false start before "me suis dit que c'était"
- "OpenClo. Ici, tu vas devoir mettre ton identifiant" repeated as "Ici tu vas devoir mettre ton propre"

Current autotrim.py doesn't detect these as retakes because:
- They're very short (6-8 words)
- The matching block continues without a gap split
- No retake-specific logic exists in autotrim.py

### Issue 2: Difflib Alignment Error  
The phrase "les cron jobs, c'est-à-dire" exists in both raw and expected, but difflib matched the raw segment to the WRONG part of expected. This causes a 1.6s error.

Possible causes:
- min_block_size=2 is too low (allows very small matches that create misalignments)
- The phrase is only 5 words, not enough for confident matching
- Surrounding context has many similar phrases

## Proposed Fixes

### Fix 1: Detect Short Retakes
Add logic to detect when a short segment is immediately followed by a very similar (but longer) segment:
- Compare word overlap between adjacent segments
- If overlap > 70% and next segment is longer, remove the shorter one
- This catches false starts and immediate retakes

### Fix 2: Increase min_block_size
Change min_block_size from 2 to 3 or 4:
- This forces difflib to find longer, more confident matches
- Reduces false matches of very short phrases
- Should improve alignment accuracy

### Fix 3: Adjust max_internal_gap
Current: 1500ms. Try 1200ms or 1000ms:
- Splits blocks more aggressively at silences
- Helps separate retakes that have a brief pause

### Fix 4: Post-process to Remove Micro-segments
After segmentation, remove segments that are:
- < 3 seconds long
- < 10 words
- Not connected to adjacent segments (gap > 2s)

## Implementation Plan

1. **Test parameter changes** (quick, low-risk):
   - min_block_size: 2 → 3 or 4
   - max_internal_gap: 1500ms → 1200ms
   - See if this alone fixes the issues

2. **Add retake detection** (medium effort):
   - After merge_segments, scan for adjacent similar segments
   - Remove shorter duplicates

3. **Add micro-segment cleanup** (low effort):
   - After all processing, remove tiny segments

4. **Re-run and measure**:
   - Generate new output.mp4
   - Transcribe (or use existing if transcription is deterministic)
   - Measure new similarity score

## Expected Outcome

If we fix the 2 retakes (3s) and improve alignment:
- Remove: 3s of wrong content
- Potential to recover: 1.6s of missing content
- Net improvement: ~4.6s closer to target

Current gap: 3.45 percentage points
Expected gap after: ~2-2.5 percentage points (bringing us to ~97-97.5%)

To reach 99%, we may need multiple iterations.
