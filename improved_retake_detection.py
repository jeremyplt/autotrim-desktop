#!/usr/bin/env python3
"""
Improved retake detection using n-gram similarity and content overlap.

This script implements a more sophisticated retake detection algorithm
that can catch retakes even when the speaker rephrases.
"""

import json
import re
from pathlib import Path
from collections import Counter
import difflib

TEST_DIR = Path('/root/.openclaw/workspace/autotrim-desktop/test_data')
REPORTS_DIR = TEST_DIR / 'reports'

def normalize_text(text):
    """Normalize text for comparison."""
    text = text.lower()
    # Remove punctuation
    text = re.sub(r'[^\w\s]', '', text)
    # Normalize whitespace
    text = ' '.join(text.split())
    return text

def get_ngrams(text, n=3):
    """Extract n-grams from text."""
    words = text.split()
    return [tuple(words[i:i+n]) for i in range(len(words) - n + 1)]

def ngram_similarity(text1, text2, n=3):
    """Calculate n-gram similarity between two texts."""
    ngrams1 = set(get_ngrams(normalize_text(text1), n))
    ngrams2 = set(get_ngrams(normalize_text(text2), n))
    
    if not ngrams1 or not ngrams2:
        return 0.0
    
    intersection = len(ngrams1 & ngrams2)
    union = len(ngrams1 | ngrams2)
    
    return intersection / union if union > 0 else 0.0

def sequence_matcher_similarity(text1, text2):
    """Use difflib to calculate sequence similarity."""
    return difflib.SequenceMatcher(None, normalize_text(text1), normalize_text(text2)).ratio()

def detect_retake_groups_advanced(chunks, time_window=180.0, min_similarity=0.28):
    """
    Detect retake groups using content similarity.
    
    Returns list of retake groups, where each group is a list of chunk IDs
    that are retakes of each other. The LAST chunk in each group should be kept.
    """
    retake_groups = []
    processed = set()
    
    for i, chunk_i in enumerate(chunks):
        if chunk_i['id'] in processed:
            continue
        
        # Look for similar chunks that come AFTER this one within the time window
        group = [chunk_i['id']]
        
        for j in range(i + 1, len(chunks)):
            chunk_j = chunks[j]
            
            if chunk_j['id'] in processed:
                continue
            
            # Check if within time window
            if chunk_j['start'] - chunk_i['end'] > time_window:
                break
            
            # Calculate similarity
            ngram_sim = ngram_similarity(chunk_i['text'], chunk_j['text'], n=3)
            seq_sim = sequence_matcher_similarity(chunk_i['text'], chunk_j['text'])
            
            # Use max of the two similarities
            similarity = max(ngram_sim, seq_sim)
            
            if similarity >= min_similarity:
                group.append(chunk_j['id'])
                processed.add(chunk_j['id'])
        
        if len(group) > 1:
            retake_groups.append(group)
            processed.add(chunk_i['id'])
    
    return retake_groups

def build_advanced_hints(chunks):
    """
    Build retake hints using advanced detection.
    """
    retake_groups = detect_retake_groups_advanced(chunks, time_window=180.0, min_similarity=0.35)
    
    if not retake_groups:
        return ""
    
    hints = ["## REPRISES PRÉ-DÉTECTÉES (DÉTECTION AVANCÉE)\n"]
    hints.append("Ces groupes ont été détectés algorithmiquement comme des REPRISES (même contenu répété).\n")
    hints.append("Pour chaque groupe, garde UNIQUEMENT le DERNIER chunk indiqué.\n\n")
    
    for group_id, group in enumerate(retake_groups):
        # Get chunk texts for preview
        chunk_texts = []
        for cid in group:
            if cid < len(chunks):
                text = chunks[cid]['text'][:60] + ('...' if len(chunks[cid]['text']) > 60 else '')
                chunk_texts.append(f"  [{cid}] {text}")
        
        last_chunk_id = group[-1]
        remove_ids = group[:-1]
        
        hints.append(f"⚠️ GROUPE DE REPRISES #{group_id + 1}:")
        hints.append(f"   Chunks: {group}")
        hints.append(f"   → GARDER SEULEMENT: [{last_chunk_id}]")
        hints.append(f"   → SUPPRIMER: {remove_ids}")
        hints.append("")
        for text in chunk_texts:
            hints.append(text)
        hints.append("")
    
    return "\n".join(hints) + "\n"

def analyze_detection_quality(chunks, python_segments):
    """
    Analyze how well the advanced detection performs compared to ground truth.
    """
    retake_groups = detect_retake_groups_advanced(chunks, time_window=180.0, min_similarity=0.35)
    
    # Build the keep set based on retake groups
    remove_set = set()
    for group in retake_groups:
        # Remove all but the last one
        remove_set.update(group[:-1])
    
    algorithmic_keep = set(range(len(chunks))) - remove_set
    
    # Ground truth: map Python segments to chunks
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
    
    # Compare
    correct_keeps = algorithmic_keep & should_keep
    false_positives = algorithmic_keep - should_keep
    false_negatives = should_keep - algorithmic_keep
    
    precision = len(correct_keeps) / len(algorithmic_keep) if algorithmic_keep else 0
    recall = len(correct_keeps) / len(should_keep) if should_keep else 0
    f1 = 2 * precision * recall / (precision + recall) if (precision + recall) > 0 else 0
    
    print(f"\n{'='*80}")
    print(f"ADVANCED RETAKE DETECTION PERFORMANCE")
    print(f"{'='*80}")
    print(f"Detected {len(retake_groups)} retake groups")
    print(f"Would keep: {len(algorithmic_keep)} chunks")
    print(f"Should keep (ground truth): {len(should_keep)} chunks")
    print(f"\nPrecision: {precision*100:.1f}% (of what we keep, how much is correct)")
    print(f"Recall: {recall*100:.1f}% (of what should be kept, how much do we catch)")
    print(f"F1 Score: {f1*100:.1f}%")
    print(f"\nFalse positives: {len(false_positives)} (keep but shouldn't)")
    print(f"False negatives: {len(false_negatives)} (remove but shouldn't)")
    
    # Show some false positives
    if false_positives:
        print(f"\n{'='*80}")
        print(f"EXAMPLES OF CHUNKS ALGORITHM KEEPS BUT SHOULDN'T (first 10):")
        print(f"{'='*80}")
        for cid in sorted(false_positives)[:10]:
            if cid < len(chunks):
                chunk = chunks[cid]
                preview = chunk['text'][:100] + ('...' if len(chunk['text']) > 100 else '')
                print(f"\n[{cid}] {chunk['start']:.1f}s-{chunk['end']:.1f}s")
                print(f"  {preview}")
    
    return {
        'retake_groups': len(retake_groups),
        'algorithmic_keep': len(algorithmic_keep),
        'should_keep': len(should_keep),
        'precision': precision,
        'recall': recall,
        'f1': f1,
        'false_positives': len(false_positives),
        'false_negatives': len(false_negatives),
    }

def main():
    # Load data
    chunks = json.load(open(REPORTS_DIR / 'rust_sim_chunks.json'))
    python_segments = json.load(open(REPORTS_DIR / 'segments.json'))
    
    print(f"Loaded {len(chunks)} chunks")
    print(f"Loaded {len(python_segments)} Python segments (ground truth)")
    
    # Analyze different similarity thresholds
    print(f"\n{'='*80}")
    print(f"TESTING DIFFERENT SIMILARITY THRESHOLDS")
    print(f"{'='*80}")
    
    for threshold in [0.25, 0.30, 0.35, 0.40, 0.45]:
        print(f"\n--- Threshold: {threshold} ---")
        
        retake_groups = detect_retake_groups_advanced(chunks, time_window=180.0, min_similarity=threshold)
        remove_set = set()
        for group in retake_groups:
            remove_set.update(group[:-1])
        
        algorithmic_keep = set(range(len(chunks))) - remove_set
        
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
        
        correct_keeps = algorithmic_keep & should_keep
        false_positives = algorithmic_keep - should_keep
        false_negatives = should_keep - algorithmic_keep
        
        precision = len(correct_keeps) / len(algorithmic_keep) if algorithmic_keep else 0
        recall = len(correct_keeps) / len(should_keep) if should_keep else 0
        f1 = 2 * precision * recall / (precision + recall) if (precision + recall) > 0 else 0
        
        kept_duration = sum(chunks[i]['end'] - chunks[i]['start'] for i in algorithmic_keep if i < len(chunks))
        
        print(f"Groups: {len(retake_groups)}, Keep: {len(algorithmic_keep)}, Duration: {kept_duration/60:.1f}min")
        print(f"Precision: {precision*100:.1f}%, Recall: {recall*100:.1f}%, F1: {f1*100:.1f}%")
        print(f"FP: {len(false_positives)}, FN: {len(false_negatives)}")
    
    # Use best threshold
    print(f"\n{'='*80}")
    print(f"GENERATING HINTS WITH THRESHOLD 0.35")
    print(f"{'='*80}")
    
    hints = build_advanced_hints(chunks)
    
    # Save hints
    hints_path = REPORTS_DIR / 'advanced_retake_hints.txt'
    with open(hints_path, 'w') as f:
        f.write(hints)
    print(f"\nAdvanced hints saved to {hints_path}")
    
    print(hints[:2000] + "\n..." if len(hints) > 2000 else hints)

if __name__ == '__main__':
    main()
