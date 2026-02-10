#!/usr/bin/env python3
"""
AutoTrim v2 - Iteration 2 improvements
Enhanced retake detection and alignment
"""

# This file will contain iteration 2 improvements
# Key changes from v1:
# 1. Multi-segment retake lookahead
# 2. First-word pattern matching
# 3. Adaptive threshold based on segment length
# 4. Time-window based similarity

import sys
sys.path.insert(0, '.')
from autotrim import *

def detect_retakes_v2(segments, raw_words, base_threshold=0.55):
    """
    Enhanced retake detection with multi-segment lookahead.
    
    Improvements:
    - Looks ahead 2-3 segments (not just adjacent)
    - Lower threshold for very short segments
    - Checks first 3-4 words specifically
    - Time window: segments within 10s are compared more aggressively
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
    log(f"  Removed {len(to_remove)} retakes (v2), {len(result)} segments remain")
    return result

# Monkey-patch the original function for testing
original_detect = detect_and_remove_retakes

def use_v2_detection():
    """Switch to v2 detection"""
    global detect_and_remove_retakes
    detect_and_remove_retakes = detect_retakes_v2
    log("Using v2 retake detection (multi-segment, adaptive threshold)")

if __name__ == '__main__':
    # Test both versions
    print("AutoTrim v2 - Enhanced detection")
    print("Run with --v2 flag to use enhanced detection")
    
    if '--v2' in sys.argv:
        use_v2_detection()
        sys.argv.remove('--v2')
    
    # Import and run main from autotrim
    from autotrim import main as autotrim_main
    autotrim_main()
