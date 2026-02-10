#!/usr/bin/env python3
"""Analyze raw vs expected transcriptions to understand the editing pattern."""
import json
import difflib

def load_words(filepath):
    with open(filepath) as f:
        data = json.load(f)
    return data.get('words', [])

def load_text(filepath):
    with open(filepath) as f:
        data = json.load(f)
    return data.get('text', '')

def words_to_sentences(words, gap_threshold=1500):
    """Group words into sentences based on gaps and punctuation."""
    sentences = []
    current = []
    for w in words:
        current.append(w)
        # End sentence on period, question mark, or long gap
        text = w.get('text', '')
        if text.endswith(('.', '?', '!')) or (len(current) > 1 and current[-1]['start'] - current[-2]['end'] > gap_threshold):
            sentences.append({
                'text': ' '.join(ww['text'] for ww in current),
                'start': current[0]['start'],
                'end': current[-1]['end'],
                'word_count': len(current)
            })
            current = []
    if current:
        sentences.append({
            'text': ' '.join(ww['text'] for ww in current),
            'start': current[0]['start'],
            'end': current[-1]['end'],
            'word_count': len(current)
        })
    return sentences

def find_silences(words, threshold_ms=2000):
    """Find gaps between words that are longer than threshold."""
    silences = []
    for i in range(1, len(words)):
        gap = words[i]['start'] - words[i-1]['end']
        if gap > threshold_ms:
            silences.append({
                'start': words[i-1]['end'],
                'end': words[i]['start'],
                'duration_ms': gap,
                'before_word': words[i-1]['text'],
                'after_word': words[i]['text'],
            })
    return silences

def main():
    raw_words = load_words('raw_transcription.json')
    exp_words = load_words('expected_transcription.json')
    raw_text = load_text('raw_transcription.json')
    exp_text = load_text('expected_transcription.json')
    
    print("=" * 60)
    print("TRANSCRIPTION ANALYSIS")
    print("=" * 60)
    
    print(f"\nRaw: {len(raw_words)} words, duration {raw_words[-1]['end']/1000:.0f}s")
    print(f"Expected: {len(exp_words)} words, duration {exp_words[-1]['end']/1000:.0f}s")
    print(f"Compression ratio: {len(exp_words)/len(raw_words)*100:.1f}% of words kept")
    
    # Find silences in raw
    silences = find_silences(raw_words, threshold_ms=2000)
    print(f"\nSilences in raw (>2s): {len(silences)}")
    total_silence = sum(s['duration_ms'] for s in silences) / 1000
    print(f"Total silence time: {total_silence:.0f}s ({total_silence/60:.1f}min)")
    
    # Show top silences
    silences_sorted = sorted(silences, key=lambda s: s['duration_ms'], reverse=True)
    print(f"\nTop 10 longest silences:")
    for s in silences_sorted[:10]:
        print(f"  {s['start']/1000:.1f}s - {s['end']/1000:.1f}s ({s['duration_ms']/1000:.1f}s) '{s['before_word']}' → '{s['after_word']}'")
    
    # Group words into sentences
    raw_sentences = words_to_sentences(raw_words)
    exp_sentences = words_to_sentences(exp_words)
    print(f"\nRaw sentences: {len(raw_sentences)}")
    print(f"Expected sentences: {len(exp_sentences)}")
    
    # Use difflib to align raw and expected text
    raw_lines = [s['text'] for s in raw_sentences]
    exp_lines = [s['text'] for s in exp_sentences]
    
    # Use SequenceMatcher on the full text to find matching blocks
    sm = difflib.SequenceMatcher(None, raw_text.split(), exp_text.split())
    ratio = sm.ratio()
    print(f"\nText similarity ratio: {ratio:.3f}")
    
    # Find matching blocks
    blocks = sm.get_matching_blocks()
    print(f"\nMatching blocks: {len(blocks)}")
    
    # Show which raw sentences are kept
    # For each expected sentence, find the best matching raw sentence
    print("\n" + "=" * 60)
    print("SEGMENT MAPPING (expected → raw)")
    print("=" * 60)
    
    # Simple word-level alignment
    raw_word_texts = [w['text'].lower() for w in raw_words]
    exp_word_texts = [w['text'].lower() for w in exp_words]
    
    # Use SequenceMatcher on word sequences
    sm2 = difflib.SequenceMatcher(None, raw_word_texts, exp_word_texts)
    matching_blocks = sm2.get_matching_blocks()
    
    print(f"\nWord-level matching blocks: {len(matching_blocks)}")
    kept_ranges = []
    for block in matching_blocks:
        if block.size > 3:  # Ignore tiny matches
            raw_start_word = raw_words[block.a]
            raw_end_word = raw_words[block.a + block.size - 1]
            exp_start_word = exp_words[block.b]
            exp_end_word = exp_words[block.b + block.size - 1]
            
            kept_ranges.append({
                'raw_start': raw_start_word['start'],
                'raw_end': raw_end_word['end'],
                'exp_start': exp_start_word['start'],
                'exp_end': exp_end_word['end'],
                'size': block.size,
                'raw_idx': block.a,
                'text_preview': ' '.join(raw_word_texts[block.a:block.a+min(8, block.size)])
            })
            print(f"  Raw[{block.a}:{block.a+block.size}] → Exp[{block.b}:{block.b+block.size}] "
                  f"({block.size} words) "
                  f"raw_time={raw_start_word['start']/1000:.1f}-{raw_end_word['end']/1000:.1f}s "
                  f"'{' '.join(raw_word_texts[block.a:block.a+min(6, block.size)])}...'")
    
    # Calculate total kept time
    total_kept = sum(r['raw_end'] - r['raw_start'] for r in kept_ranges)
    print(f"\nTotal kept time from matching: {total_kept/1000:.0f}s ({total_kept/60000:.1f}min)")
    
    # Save the analysis
    analysis = {
        'raw_words': len(raw_words),
        'exp_words': len(exp_words),
        'silences': silences,
        'kept_ranges': kept_ranges,
        'matching_blocks': [{'a': b.a, 'b': b.b, 'size': b.size} for b in matching_blocks],
        'text_similarity': ratio
    }
    with open('reports/analysis.json', 'w') as f:
        json.dump(analysis, f, indent=2, ensure_ascii=False)
    print("\nSaved analysis to reports/analysis.json")

if __name__ == '__main__':
    main()
