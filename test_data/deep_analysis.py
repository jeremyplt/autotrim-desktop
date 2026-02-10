#!/usr/bin/env python3
"""
Deep dive analysis: Why are those 5 passages different?
Let's find them in the raw transcription and understand what happened.
"""
import json
import re
import difflib

def normalize_word(text):
    return re.sub(r'[^a-z0-9àâäéèêëïîôùûüÿçœæ]', '', text.lower())

def load_trans(path):
    with open(path) as f:
        return json.load(f)

def find_passage_in_raw(passage_words, raw_words):
    """Find where a passage from output/expected appears in raw."""
    norm_passage = [normalize_word(w) for w in passage_words]
    norm_raw = [normalize_word(w['text']) for w in raw_words]
    
    # Use difflib to find the best match
    sm = difflib.SequenceMatcher(None, norm_passage, norm_raw, autojunk=False)
    match = sm.find_longest_match(0, len(norm_passage), 0, len(norm_raw))
    
    if match.size >= len(norm_passage) * 0.8:  # At least 80% match
        return match.b, match.b + match.size
    return None, None

def format_time(ms):
    secs = ms / 1000.0
    mins = int(secs // 60)
    secs = secs % 60
    return f"{mins}:{secs:06.3f}"

def get_context(words, start_idx, end_idx, context_words=20):
    """Get context around a passage."""
    before_start = max(0, start_idx - context_words)
    after_end = min(len(words), end_idx + context_words)
    
    before = ' '.join([w['text'] for w in words[before_start:start_idx]])
    passage = ' '.join([w['text'] for w in words[start_idx:end_idx]])
    after = ' '.join([w['text'] for w in words[end_idx:after_end]])
    
    return before, passage, after

def main():
    print("=" * 100)
    print("DEEP ANALYSIS: Understanding the 5 problematic passages")
    print("=" * 100)
    print()
    
    # Load all transcriptions
    raw = load_trans('raw_transcription.json')
    output = load_trans('output_transcription.json')
    expected = load_trans('expected_transcription.json')
    
    raw_words = raw['words']
    output_words = output['words']
    expected_words = expected['words']
    
    # Load the detailed analysis
    with open('reports/detailed_analysis.json') as f:
        analysis = json.load(f)
    
    print("PART 1: EXTRA CONTENT IN OUTPUT (should have been removed)")
    print("=" * 100)
    print()
    
    for i, problem in enumerate(analysis['problems']['extra_in_output'], 1):
        print(f"Problem {i}: {problem['output_time']} ({problem['duration_s']:.1f}s, {problem['word_count']} words)")
        print(f"Text: {problem['output_text']}")
        print()
        
        # Get the words from output
        o1, o2 = problem['output_range']
        passage_words = [w['text'] for w in output_words[o1:o2]]
        
        # Find in raw
        raw_start, raw_end = find_passage_in_raw(passage_words, raw_words)
        if raw_start is not None:
            print(f"Found in RAW at word indices {raw_start}-{raw_end}")
            print(f"RAW time: {format_time(raw_words[raw_start]['start'])} - {format_time(raw_words[raw_end-1]['end'])}")
            
            before, passage, after = get_context(raw_words, raw_start, raw_end, 15)
            print(f"\nContext in RAW:")
            print(f"  BEFORE: ...{before}")
            print(f"  >>> PASSAGE: {passage}")
            print(f"  AFTER: {after}...")
            print()
        
        # Check if it's in expected
        exp_start, exp_end = find_passage_in_raw(passage_words, expected_words)
        if exp_start is not None:
            print(f"❌ ERROR: This passage IS in expected! Should have been kept.")
            print(f"Expected time: {format_time(expected_words[exp_start]['start'])} - {format_time(expected_words[exp_end-1]['end'])}")
        else:
            print(f"✅ CORRECT: This passage is NOT in expected (it's a retake/error)")
            print(f"Analysis: autotrim.py should remove this but didn't")
        
        print()
        print("-" * 100)
        print()
    
    print()
    print("PART 2: MISSING CONTENT FROM OUTPUT (should have been kept)")
    print("=" * 100)
    print()
    
    for i, problem in enumerate(analysis['problems']['missing_from_output'], 1):
        print(f"Problem {i}: {problem['expected_time']} ({problem['duration_s']:.1f}s, {problem['word_count']} words)")
        print(f"Text: {problem['expected_text']}")
        print()
        
        # Get the words from expected
        e1, e2 = problem['expected_range']
        passage_words = [w['text'] for w in expected_words[e1:e2]]
        
        # Find in raw
        raw_start, raw_end = find_passage_in_raw(passage_words, raw_words)
        if raw_start is not None:
            print(f"Found in RAW at word indices {raw_start}-{raw_end}")
            print(f"RAW time: {format_time(raw_words[raw_start]['start'])} - {format_time(raw_words[raw_end-1]['end'])}")
            
            before, passage, after = get_context(raw_words, raw_start, raw_end, 15)
            print(f"\nContext in RAW:")
            print(f"  BEFORE: ...{before}")
            print(f"  >>> PASSAGE: {passage}")
            print(f"  AFTER: {after}...")
            print()
            
            print(f"✅ This passage exists in RAW and should be in output")
            print(f"Analysis: autotrim.py removed this passage incorrectly")
        else:
            print(f"❌ WARNING: Cannot find this passage in RAW!")
            print(f"This might be a transcription mismatch issue")
        
        print()
        print("-" * 100)
        print()
    
    # Now let's understand the difflib matching behavior
    print()
    print("PART 3: DIFFLIB ANALYSIS")
    print("=" * 100)
    print()
    
    raw_texts = [normalize_word(w['text']) for w in raw_words]
    exp_texts = [normalize_word(w['text']) for w in expected_words]
    
    sm = difflib.SequenceMatcher(None, raw_texts, exp_texts, autojunk=False)
    blocks = sm.get_matching_blocks()
    
    print(f"Total matching blocks: {len(blocks)}")
    print(f"Total matched words: {sum(b.size for b in blocks)}")
    print()
    
    # Check if the problematic passages are in the matching blocks
    print("Checking if problematic passages are in difflib matches...")
    print()
    
    for problem in analysis['problems']['missing_from_output']:
        e1, e2 = problem['expected_range']
        passage_words = [expected_words[i]['text'] for i in range(e1, min(e2, e1+5))]
        
        raw_start, raw_end = find_passage_in_raw(passage_words, raw_words)
        if raw_start is not None:
            # Check if this range is covered by a matching block
            covered = False
            for block in blocks:
                if block.a <= raw_start < block.a + block.size and block.b <= e1 < block.b + block.size:
                    covered = True
                    break
            
            if covered:
                print(f"✅ Passage '{problem['expected_text'][:50]}...' IS covered by a difflib match")
                print(f"   → autotrim.py should have kept it. Likely removed by gap splitting.")
            else:
                print(f"❌ Passage '{problem['expected_text'][:50]}...' NOT covered by difflib")
                print(f"   → difflib itself failed to match this passage")
        print()

if __name__ == '__main__':
    main()
