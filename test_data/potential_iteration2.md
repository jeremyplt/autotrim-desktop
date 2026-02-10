# Potential Iteration 2 Improvements

## If Iteration 1 doesn't reach 99%

### Strategy 1: More Aggressive Retake Detection

Current retake detector only looks at adjacent segments with >70% overlap.

**Improvements:**
- Look ahead 2-3 segments (not just next)
- Lower threshold to 50-60% for very short segments (<8 words)
- Check first 3-4 words specifically (common retake pattern: same opening, different ending)
- Add time window: if segments start within 10s, more aggressive matching

**Example pattern to catch:**
```
Segment A: "Ici tu vas devoir mettre ton" (6 words)
[gap 2s]
Segment B: "Ici tu vas devoir mettre ton propre identifiant" (9 words)
→ Remove A, keep B
```

### Strategy 2: Context-Aware Difflib Matching

Current issue: difflib sometimes matches phrases to the wrong location.

**Improvements:**
- After initial alignment, verify each match with surrounding context
- If a 5-word phrase matches but the 10 words before/after DON'T match, flag as suspicious
- Re-align suspicious regions with stricter parameters (min_block_size=5 or higher)
- Use local alignment instead of global for ambiguous phrases

### Strategy 3: Two-Pass Alignment

**Pass 1: Anchor matching**
- Find long, unique phrases (>20 words, min_block_size=10)
- These are "anchors" that are definitely correct

**Pass 2: Fill gaps**
- Between anchors, use more lenient matching (min_block_size=3)
- This prevents misalignment across long distances

### Strategy 4: Phrase Frequency Analysis

**Problem:** Common phrases like "et puis", "donc voilà" appear many times
**Solution:**
- Before alignment, identify frequently-repeated phrases
- Weight them lower in difflib matching (autojunk=True or custom scoring)
- Focus on unique, content-bearing phrases for alignment

### Strategy 5: Manual Override for Known Issues

If we can't algorithmically fix the 2-3 remaining problematic passages:
- Add a "known_issues.json" file with specific time ranges to remove/keep
- Apply these as a final post-processing step
- Document why each override is needed

## Parameter Tuning Options

If current parameters (min_words=3, max_internal_gap=1200) aren't enough:

### Option A: More Conservative (Better Alignment)
- min_words=4 or 5
- max_internal_gap=1000ms
- merge_gap=300ms (less aggressive merging)

### Option B: More Aggressive (Remove More)
- min_words=2 (back to original)
- max_internal_gap=1000ms (split more)
- merge_gap=1000ms (merge more aggressively, catching retakes in merges)
- min_duration for micro-segments=3000ms (remove larger fragments)

### Option C: Hybrid
- Use Option A for initial alignment (conservative, accurate)
- Then apply aggressive retake detection
- Then merge with Option B parameters

## Testing Strategy

For each iteration:
1. Dry-run to check segment count and duration
2. If promising (duration within 2% of target), render full
3. Transcribe new output (or use proxy analysis if no transcription available)
4. Measure similarity
5. Analyze remaining errors
6. Design targeted fix for the specific errors
7. Repeat

## When to Stop Iterating

- **Success:** Similarity ≥ 99% AND manual review confirms quality
- **Diminishing returns:** 3+ iterations with <0.5% improvement each
- **Root cause unfixable:** Remaining errors are due to transcription mismatches (not our fault)

## Estimated Iterations Needed

- Baseline: 95.55%
- After Iteration 1: ~96.5-97.5% (estimated)
- After Iteration 2: ~98-98.5%
- After Iteration 3: ~99%+

Total: 3-4 iterations to reach 99%.
