#!/usr/bin/env python3
"""
Quick analysis without full transcription:
Compare segments.json with expected to estimate match quality.
"""
import json
import difflib
import re

def normalize_word(text):
    return re.sub(r'[^a-z0-9àâäéèêëïîôùûüÿçœæ]', '', text.lower())

def main():
    print("=" * 80)
    print("QUICK ANALYSIS - Segment-based comparison")
    print("=" * 80)
    print()
    
    # Load data
    with open('reports/segments.json') as f:
        segments = json.load(f)
    
    with open('raw_transcription.json') as f:
        raw_trans = json.load(f)
    
    with open('expected_transcription.json') as f:
        exp_trans = json.load(f)
    
    raw_words = raw_trans['words']
    exp_words = exp_trans['words']
    
    # Calculate what words will be in the output
    output_word_indices = set()
    for seg in segments:
        for i in range(seg['raw_start_idx'], seg['raw_end_idx'] + 1):
            output_word_indices.add(i)
    
    # Get output words
    output_words_from_raw = [raw_words[i] for i in sorted(output_word_indices)]
    
    # Normalize for comparison
    output_texts = [normalize_word(w['text']) for w in output_words_from_raw]
    exp_texts = [normalize_word(w['text']) for w in exp_words]
    
    # Filter empty
    output_texts = [w for w in output_texts if w]
    exp_texts = [w for w in exp_texts if w]
    
    # Calculate similarity
    sm = difflib.SequenceMatcher(None, output_texts, exp_texts, autojunk=False)
    matches = sum(block.size for block in sm.get_matching_blocks())
    
    precision = matches / len(output_texts) if output_texts else 0
    recall = matches / len(exp_texts) if exp_texts else 0
    f1 = 2 * precision * recall / (precision + recall) if (precision + recall) > 0 else 0
    similarity = (matches / max(len(output_texts), len(exp_texts))) * 100 if max(len(output_texts), len(exp_texts)) > 0 else 0
    
    # Duration
    total_duration = sum((s['raw_end_ms'] - s['raw_start_ms'])/1000.0 for s in segments)
    expected_duration = exp_words[-1]['end'] / 1000.0
    
    print(f"Segments: {len(segments)}")
    print(f"Output words: {len(output_texts)}")
    print(f"Expected words: {len(exp_texts)}")
    print(f"Matched words: {matches}")
    print()
    print(f"Precision: {precision*100:.2f}%")
    print(f"Recall: {recall*100:.2f}%")
    print(f"F1 Score: {f1*100:.2f}%")
    print(f"Similarity: {similarity:.2f}%")
    print()
    print(f"Duration: {total_duration:.1f}s ({total_duration/60:.2f} min)")
    print(f"Expected: {expected_duration:.1f}s ({expected_duration/60:.2f} min)")
    print(f"Ratio: {(total_duration/expected_duration)*100:.1f}%")
    print()
    
    if similarity >= 99.0:
        print("✅ TARGET REACHED: Similarity >= 99%!")
    elif similarity >= 97.0:
        print(f"🟡 CLOSE: {99.0 - similarity:.2f} percentage points from target")
    else:
        print(f"❌ NEED MORE WORK: {99.0 - similarity:.2f} percentage points from target")
    
    # Save quick report
    report = {
        'segments': len(segments),
        'output_words': len(output_texts),
        'expected_words': len(exp_texts),
        'matched_words': matches,
        'precision': precision,
        'recall': recall,
        'f1': f1,
        'similarity_pct': similarity,
        'duration_s': total_duration,
        'expected_duration_s': expected_duration,
        'duration_ratio': total_duration / expected_duration
    }
    
    with open('reports/quick_analysis.json', 'w') as f:
        json.dump(report, f, indent=2)
    
    print()
    print("Report saved to: reports/quick_analysis.json")

if __name__ == '__main__':
    main()
