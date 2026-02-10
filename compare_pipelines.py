#!/usr/bin/env python3
"""
Compare what the Rust/Claude pipeline kept vs what the Python/difflib pipeline kept.
"""

import json
from pathlib import Path

TEST_DIR = Path('/root/.openclaw/workspace/autotrim-desktop/test_data')
REPORTS_DIR = TEST_DIR / 'reports'

def load_json(path):
    with open(path) as f:
        return json.load(f)

def main():
    # Load Rust chunks
    rust_chunks = load_json(REPORTS_DIR / 'rust_sim_chunks.json')
    rust_keep_ids = load_json(REPORTS_DIR / 'rust_sim_keep_ids.json')
    
    # Load Python segments (what SHOULD be kept)
    python_segments = load_json(REPORTS_DIR / 'segments.json')
    
    print(f"Rust chunks: {len(rust_chunks)}")
    print(f"Rust kept: {len(rust_keep_ids)}")
    print(f"Python segments: {len(python_segments)}")
    
    # Map Python segments to Rust chunks
    # A Rust chunk should be kept if it overlaps significantly with any Python segment
    rust_keep_set = set(rust_keep_ids)
    
    should_keep = set()
    for seg in python_segments:
        seg_start = seg['raw_start_ms'] / 1000.0
        seg_end = seg['raw_end_ms'] / 1000.0
        
        # Find all chunks that overlap with this segment
        for chunk in rust_chunks:
            # Check if there's significant overlap
            overlap_start = max(chunk['start'], seg_start)
            overlap_end = min(chunk['end'], seg_end)
            overlap_duration = max(0, overlap_end - overlap_start)
            
            chunk_duration = chunk['end'] - chunk['start']
            overlap_ratio = overlap_duration / chunk_duration if chunk_duration > 0 else 0
            
            # If at least 50% of the chunk overlaps with the segment, it should be kept
            if overlap_ratio >= 0.5:
                should_keep.add(chunk['id'])
    
    print(f"\nChunks that SHOULD be kept (based on Python): {len(should_keep)}")
    print(f"Chunks that Claude KEPT: {len(rust_keep_set)}")
    
    # Find errors
    false_positives = rust_keep_set - should_keep  # Claude kept but shouldn't
    false_negatives = should_keep - rust_keep_set  # Claude removed but shouldn't
    
    print(f"\n❌ FALSE POSITIVES (Claude kept, but should remove): {len(false_positives)}")
    print(f"❌ FALSE NEGATIVES (Claude removed, but should keep): {len(false_negatives)}")
    
    # Calculate duration impact
    fp_duration = sum(
        rust_chunks[i]['end'] - rust_chunks[i]['start']
        for i in false_positives if i < len(rust_chunks)
    )
    
    fn_duration = sum(
        rust_chunks[i]['end'] - rust_chunks[i]['start']
        for i in false_negatives if i < len(rust_chunks)
    )
    
    print(f"\nFalse positive duration: {fp_duration:.1f}s ({fp_duration/60:.1f}min)")
    print(f"False negative duration: {fn_duration:.1f}s ({fn_duration/60:.1f}min)")
    print(f"Net excess: {fp_duration - fn_duration:.1f}s ({(fp_duration - fn_duration)/60:.1f}min)")
    
    # Show examples of false positives (bad takes Claude kept)
    print(f"\n{'='*80}")
    print(f"EXAMPLES OF BAD TAKES CLAUDE KEPT (first 20):")
    print(f"{'='*80}")
    
    fp_list = sorted(false_positives)[:20]
    for i in fp_list:
        if i < len(rust_chunks):
            chunk = rust_chunks[i]
            preview = chunk['text'][:100] + ('...' if len(chunk['text']) > 100 else '')
            print(f"\n[{i}] {chunk['start']:.1f}s-{chunk['end']:.1f}s ({chunk['end']-chunk['start']:.1f}s)")
            print(f"  {preview}")
    
    # Show examples of false negatives (good content Claude removed)
    print(f"\n{'='*80}")
    print(f"EXAMPLES OF GOOD CONTENT CLAUDE REMOVED (first 10):")
    print(f"{'='*80}")
    
    fn_list = sorted(false_negatives)[:10]
    for i in fn_list:
        if i < len(rust_chunks):
            chunk = rust_chunks[i]
            preview = chunk['text'][:100] + ('...' if len(chunk['text']) > 100 else '')
            print(f"\n[{i}] {chunk['start']:.1f}s-{chunk['end']:.1f}s ({chunk['end']-chunk['start']:.1f}s)")
            print(f"  {preview}")
    
    # Save detailed analysis
    analysis = {
        'rust_chunks_total': len(rust_chunks),
        'rust_kept_count': len(rust_keep_set),
        'python_segments_count': len(python_segments),
        'should_keep_count': len(should_keep),
        'false_positives_count': len(false_positives),
        'false_negatives_count': len(false_negatives),
        'false_positive_duration': fp_duration,
        'false_negative_duration': fn_duration,
        'net_excess': fp_duration - fn_duration,
        'false_positive_ids': sorted(false_positives),
        'false_negative_ids': sorted(false_negatives),
    }
    
    with open(REPORTS_DIR / 'pipeline_comparison.json', 'w') as f:
        json.dump(analysis, f, indent=2)
    
    print(f"\n\nDetailed analysis saved to {REPORTS_DIR / 'pipeline_comparison.json'}")

if __name__ == '__main__':
    main()
