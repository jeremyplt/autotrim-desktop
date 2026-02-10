#!/usr/bin/env python3
"""
Detailed analysis of output vs expected transcriptions to identify problematic passages.
"""
import json
import difflib
import re
from collections import defaultdict

def normalize_word(text):
    """Normalize a word for matching: lowercase, strip punctuation."""
    return re.sub(r'[^a-z0-9àâäéèêëïîôùûüÿçœæ]', '', text.lower())

def load_transcription(path):
    """Load an AssemblyAI transcription JSON file."""
    with open(path) as f:
        return json.load(f)

def get_words(transcription):
    """Extract word list from transcription."""
    return transcription.get('words', [])

def format_time(ms):
    """Format milliseconds as MM:SS.mmm"""
    secs = ms / 1000.0
    mins = int(secs // 60)
    secs = secs % 60
    return f"{mins}:{secs:06.3f}"

def analyze_alignment(output_words, expected_words):
    """
    Detailed alignment between output and expected.
    Returns problematic passages.
    """
    out_texts = [normalize_word(w['text']) for w in output_words]
    exp_texts = [normalize_word(w['text']) for w in expected_words]
    
    sm = difflib.SequenceMatcher(None, out_texts, exp_texts, autojunk=False)
    
    problems = {
        'extra_in_output': [],  # Passages in output that shouldn't be there
        'missing_from_output': [],  # Passages from expected that are missing
        'out_of_order': []  # Passages in wrong order
    }
    
    last_exp_idx = -1
    
    for opcode, o1, o2, e1, e2 in sm.get_opcodes():
        if opcode == 'equal':
            # Check for out-of-order
            if e1 < last_exp_idx:
                problems['out_of_order'].append({
                    'output_range': (o1, o2),
                    'expected_range': (e1, e2),
                    'output_text': ' '.join([output_words[i]['text'] for i in range(o1, min(o2, o1+10))]),
                    'output_time': f"{format_time(output_words[o1]['start'])} - {format_time(output_words[o2-1]['end'])}" if o1 < o2 else "empty"
                })
            last_exp_idx = max(last_exp_idx, e2)
        
        elif opcode == 'delete':
            # Extra content in output (should have been removed)
            if o2 - o1 >= 5:  # Only report if >= 5 words
                problems['extra_in_output'].append({
                    'output_range': (o1, o2),
                    'word_count': o2 - o1,
                    'output_text': ' '.join([output_words[i]['text'] for i in range(o1, min(o2, o1+20))]),
                    'output_time': f"{format_time(output_words[o1]['start'])} - {format_time(output_words[o2-1]['end'])}",
                    'duration_s': (output_words[o2-1]['end'] - output_words[o1]['start']) / 1000.0
                })
        
        elif opcode == 'insert':
            # Missing content from output (should have been kept)
            if e2 - e1 >= 5:  # Only report if >= 5 words
                problems['missing_from_output'].append({
                    'expected_range': (e1, e2),
                    'word_count': e2 - e1,
                    'expected_text': ' '.join([expected_words[i]['text'] for i in range(e1, min(e2, e1+20))]),
                    'expected_time': f"{format_time(expected_words[e1]['start'])} - {format_time(expected_words[e2-1]['end'])}",
                    'duration_s': (expected_words[e2-1]['end'] - expected_words[e1]['start']) / 1000.0
                })
        
        elif opcode == 'replace':
            # Different content - could be either problem
            if o2 - o1 >= 5:
                problems['extra_in_output'].append({
                    'output_range': (o1, o2),
                    'word_count': o2 - o1,
                    'output_text': ' '.join([output_words[i]['text'] for i in range(o1, min(o2, o1+20))]),
                    'output_time': f"{format_time(output_words[o1]['start'])} - {format_time(output_words[o2-1]['end'])}",
                    'duration_s': (output_words[o2-1]['end'] - output_words[o1]['start']) / 1000.0,
                    'note': 'replaced content'
                })
            if e2 - e1 >= 5:
                problems['missing_from_output'].append({
                    'expected_range': (e1, e2),
                    'word_count': e2 - e1,
                    'expected_text': ' '.join([expected_words[i]['text'] for i in range(e1, min(e2, e1+20))]),
                    'expected_time': f"{format_time(expected_words[e1]['start'])} - {format_time(expected_words[e2-1]['end'])}",
                    'duration_s': (expected_words[e2-1]['end'] - expected_words[e1]['start']) / 1000.0,
                    'note': 'replaced content'
                })
    
    return problems

def calculate_similarity(output_words, expected_words):
    """Calculate word-level similarity."""
    out_texts = [normalize_word(w['text']) for w in output_words]
    exp_texts = [normalize_word(w['text']) for w in expected_words]
    
    sm = difflib.SequenceMatcher(None, out_texts, exp_texts, autojunk=False)
    matches = sum(block.size for block in sm.get_matching_blocks())
    
    precision = matches / len(out_texts) if out_texts else 0
    recall = matches / len(exp_texts) if exp_texts else 0
    f1 = 2 * precision * recall / (precision + recall) if (precision + recall) > 0 else 0
    
    return {
        'matches': matches,
        'output_words': len(out_texts),
        'expected_words': len(exp_texts),
        'precision': precision,
        'recall': recall,
        'f1': f1,
        'similarity_pct': (matches / max(len(out_texts), len(exp_texts))) * 100 if max(len(out_texts), len(exp_texts)) > 0 else 0
    }

def main():
    print("=" * 80)
    print("TRANSCRIPTION ANALYSIS - Output vs Expected")
    print("=" * 80)
    print()
    
    # Load transcriptions
    print("Loading transcriptions...")
    output_trans = load_transcription('output_transcription.json')
    expected_trans = load_transcription('expected_transcription.json')
    
    output_words = get_words(output_trans)
    expected_words = get_words(expected_trans)
    
    output_duration = output_words[-1]['end'] / 1000.0 if output_words else 0
    expected_duration = expected_words[-1]['end'] / 1000.0 if expected_words else 0
    
    print(f"Output: {len(output_words)} words, duration: {output_duration/60:.2f} min")
    print(f"Expected: {len(expected_words)} words, duration: {expected_duration/60:.2f} min")
    print()
    
    # Calculate similarity
    print("Calculating similarity...")
    sim = calculate_similarity(output_words, expected_words)
    print(f"Word matches: {sim['matches']} / {sim['expected_words']}")
    print(f"Precision: {sim['precision']*100:.2f}%")
    print(f"Recall: {sim['recall']*100:.2f}%")
    print(f"F1 Score: {sim['f1']*100:.2f}%")
    print(f"Overall similarity: {sim['similarity_pct']:.2f}%")
    print()
    
    # Analyze alignment
    print("Analyzing alignment (this may take a while)...")
    problems = analyze_alignment(output_words, expected_words)
    
    # Report problems
    print("=" * 80)
    print("PROBLEMATIC PASSAGES")
    print("=" * 80)
    print()
    
    # Extra content in output (should have been removed)
    extra = problems['extra_in_output']
    total_extra_duration = sum(p['duration_s'] for p in extra)
    print(f"1. EXTRA CONTENT IN OUTPUT (should have been removed)")
    print(f"   Count: {len(extra)} passages")
    print(f"   Total duration: {total_extra_duration:.1f}s ({total_extra_duration/60:.2f} min)")
    print()
    if extra:
        print("   Top 10 longest:")
        for i, p in enumerate(sorted(extra, key=lambda x: x['duration_s'], reverse=True)[:10], 1):
            note = f" [{p['note']}]" if 'note' in p else ""
            print(f"   {i}. {p['output_time']} ({p['duration_s']:.1f}s, {p['word_count']} words){note}")
            print(f"      {p['output_text'][:120]}...")
            print()
    
    # Missing content from output
    missing = problems['missing_from_output']
    total_missing_duration = sum(p['duration_s'] for p in missing)
    print(f"2. MISSING CONTENT FROM OUTPUT (should have been kept)")
    print(f"   Count: {len(missing)} passages")
    print(f"   Total duration: {total_missing_duration:.1f}s ({total_missing_duration/60:.2f} min)")
    print()
    if missing:
        print("   Top 10 longest:")
        for i, p in enumerate(sorted(missing, key=lambda x: x['duration_s'], reverse=True)[:10], 1):
            note = f" [{p['note']}]" if 'note' in p else ""
            print(f"   {i}. {p['expected_time']} ({p['duration_s']:.1f}s, {p['word_count']} words){note}")
            print(f"      {p['expected_text'][:120]}...")
            print()
    
    # Out of order
    ooo = problems['out_of_order']
    print(f"3. OUT OF ORDER PASSAGES")
    print(f"   Count: {len(ooo)} passages")
    if ooo:
        print()
        for i, p in enumerate(ooo[:10], 1):
            print(f"   {i}. {p['output_time']}")
            print(f"      {p['output_text'][:120]}...")
            print()
    
    # Summary
    print("=" * 80)
    print("SUMMARY")
    print("=" * 80)
    print(f"Current similarity: {sim['similarity_pct']:.2f}%")
    print(f"Target similarity: 99.00%")
    print(f"Gap: {99.0 - sim['similarity_pct']:.2f} percentage points")
    print()
    print(f"Issues to fix:")
    print(f"  - Remove {len(extra)} extra passages ({total_extra_duration/60:.2f} min)")
    print(f"  - Restore {len(missing)} missing passages ({total_missing_duration/60:.2f} min)")
    print(f"  - Fix {len(ooo)} out-of-order passages")
    print()
    
    # Save detailed report
    report = {
        'similarity': sim,
        'problems': {
            'extra_in_output': extra,
            'missing_from_output': missing,
            'out_of_order': ooo
        }
    }
    
    with open('reports/detailed_analysis.json', 'w') as f:
        json.dump(report, f, indent=2, ensure_ascii=False)
    
    print("Detailed report saved to: reports/detailed_analysis.json")
    print()
    
    return sim['similarity_pct']

if __name__ == '__main__':
    main()
