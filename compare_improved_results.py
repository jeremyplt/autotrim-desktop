#!/usr/bin/env python3
"""
Compare improved Claude results with ground truth.
"""

import json
from pathlib import Path

TEST_DIR = Path('/root/.openclaw/workspace/autotrim-desktop/test_data')
REPORTS_DIR = TEST_DIR / 'reports'

def load_json(path):
    with open(path) as f:
        return json.load(f)

def main():
    chunks = load_json(REPORTS_DIR / 'rust_sim_chunks.json')
    improved_keep_ids = load_json(REPORTS_DIR / 'rust_improved_keep_ids.json')
    python_segments = load_json(REPORTS_DIR / 'segments.json')
    
    # Ground truth
    should_keep = set()
    for seg in python_segments:
        seg_start = seg['raw_start_ms'] / 1000.0
        seg_end = seg['raw_end_ms'] / 1000.0
        
        for chunk in chunks:
            overlap_start = max(chunk['start'], seg_start)
            overlap_end = min(chunk['end'], seg_end)
            overlap_duration = max(0, overlap_end - overlap_start)
            
            chunk_duration = chunk['end'] - chunk['start']
            overlap_ratio = overlap_duration / chunk_duration if chunk_duration > 0 else 0
            
            if overlap_ratio >= 0.5:
                should_keep.add(chunk['id'])
    
    improved_keep_set = set(improved_keep_ids)
    
    # Find errors
    false_positives = improved_keep_set - should_keep
    false_negatives = should_keep - improved_keep_set
    
    print(f"{'='*80}")
    print(f"IMPROVED CLAUDE RESULTS vs GROUND TRUTH")
    print(f"{'='*80}")
    print(f"Should keep (ground truth): {len(should_keep)} chunks")
    print(f"Claude kept (improved): {len(improved_keep_set)} chunks")
    print(f"\n❌ False positives (Claude kept, shouldn't): {len(false_positives)}")
    print(f"❌ False negatives (Claude removed, shouldn't): {len(false_negatives)}")
    
    # Duration impact
    fp_duration = sum(
        chunks[i]['end'] - chunks[i]['start']
        for i in false_positives if i < len(chunks)
    )
    
    fn_duration = sum(
        chunks[i]['end'] - chunks[i]['start']
        for i in false_negatives if i < len(chunks)
    )
    
    print(f"\nFalse positive duration: {fp_duration:.1f}s ({fp_duration/60:.1f}min)")
    print(f"False negative duration: {fn_duration:.1f}s ({fn_duration/60:.1f}min)")
    print(f"Net excess: {fp_duration - fn_duration:.1f}s ({(fp_duration - fn_duration)/60:.1f}min)")
    
    # Show remaining false positives
    print(f"\n{'='*80}")
    print(f"REMAINING BAD TAKES CLAUDE STILL KEEPS (first 30):")
    print(f"{'='*80}")
    
    fp_list = sorted(false_positives)[:30]
    for i in fp_list:
        if i < len(chunks):
            chunk = chunks[i]
            preview = chunk['text'][:120] + ('...' if len(chunk['text']) > 120 else '')
            print(f"\n[{i}] {chunk['start']:.1f}s-{chunk['end']:.1f}s ({chunk['end']-chunk['start']:.1f}s, {chunk['word_count']} mots)")
            print(f"  {preview}")
    
    # Save analysis
    with open(REPORTS_DIR / 'improved_analysis.json', 'w') as f:
        json.dump({
            'should_keep_count': len(should_keep),
            'improved_keep_count': len(improved_keep_set),
            'false_positives_count': len(false_positives),
            'false_negatives_count': len(false_negatives),
            'fp_duration': fp_duration,
            'fn_duration': fn_duration,
            'net_excess': fp_duration - fn_duration,
            'false_positive_ids': sorted(false_positives),
            'false_negative_ids': sorted(false_negatives),
        }, f, indent=2)
    
    print(f"\n\nAnalysis saved to improved_analysis.json")

if __name__ == '__main__':
    main()
