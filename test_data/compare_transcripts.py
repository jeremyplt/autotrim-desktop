#!/usr/bin/env python3
"""Compare output transcription with expected transcription"""
import json
import difflib
import os
from collections import Counter

def load_words(path):
    """Load word list from transcription JSON"""
    with open(path) as f:
        data = json.load(f)
    words = data.get('words', [])
    return words

def extract_text_words(words):
    """Extract just the text from word objects, lowercased"""
    return [w['text'].lower().strip('.,!?;:()[]"\'') for w in words if w.get('text', '').strip()]

def format_time(ms):
    """Format milliseconds to mm:ss"""
    if ms is None:
        return "??:??"
    s = ms / 1000
    m = int(s // 60)
    sec = int(s % 60)
    return f"{m:02d}:{sec:02d}"

def find_sentence_chunks(words, chunk_size=20):
    """Break word list into overlapping chunks for comparison"""
    texts = [w['text'] for w in words]
    chunks = []
    for i in range(0, len(texts), chunk_size // 2):
        chunk_words = words[i:i+chunk_size]
        if chunk_words:
            chunk_text = ' '.join(w['text'] for w in chunk_words)
            start_time = chunk_words[0].get('start', 0)
            end_time = chunk_words[-1].get('end', 0)
            chunks.append({
                'text': chunk_text,
                'start': start_time,
                'end': end_time,
                'word_start_idx': i,
                'word_end_idx': i + len(chunk_words)
            })
    return chunks

def main():
    print("Loading transcriptions...")
    expected_words = load_words('expected_transcription.json')
    output_words = load_words('output_transcription.json')
    
    print(f"Expected: {len(expected_words)} words")
    print(f"Output: {len(output_words)} words")
    
    # Extract text-only words
    exp_texts = extract_text_words(expected_words)
    out_texts = extract_text_words(output_words)
    
    print(f"\nExpected text words: {len(exp_texts)}")
    print(f"Output text words: {len(out_texts)}")
    
    # Use SequenceMatcher for alignment
    print("\nComputing sequence alignment (this may take a moment)...")
    sm = difflib.SequenceMatcher(None, exp_texts, out_texts, autojunk=False)
    
    matching_blocks = sm.get_matching_blocks()
    total_matching = sum(block.size for block in matching_blocks)
    
    match_pct_exp = (total_matching / len(exp_texts)) * 100 if exp_texts else 0
    match_pct_out = (total_matching / len(out_texts)) * 100 if out_texts else 0
    
    print(f"\nMatching words: {total_matching}")
    print(f"Match % (of expected): {match_pct_exp:.1f}%")
    print(f"Match % (of output): {match_pct_out:.1f}%")
    
    # Get opcodes for detailed diff
    opcodes = sm.get_opcodes()
    
    # Categorize differences
    missing_from_output = []  # In expected but not in output
    extra_in_output = []  # In output but not in expected
    replacements = []  # Different words
    
    for tag, i1, i2, j1, j2 in opcodes:
        if tag == 'equal':
            continue
        elif tag == 'delete':
            # Words in expected missing from output
            exp_segment = expected_words[i1:i2]
            text = ' '.join(w['text'] for w in exp_segment)
            start = exp_segment[0].get('start', 0) if exp_segment else 0
            end = exp_segment[-1].get('end', 0) if exp_segment else 0
            missing_from_output.append({
                'text': text,
                'word_count': i2 - i1,
                'exp_start': start,
                'exp_end': end,
                'exp_idx': (i1, i2),
            })
        elif tag == 'insert':
            # Words in output not in expected
            out_segment = output_words[j1:j2]
            text = ' '.join(w['text'] for w in out_segment)
            start = out_segment[0].get('start', 0) if out_segment else 0
            end = out_segment[-1].get('end', 0) if out_segment else 0
            extra_in_output.append({
                'text': text,
                'word_count': j2 - j1,
                'out_start': start,
                'out_end': end,
                'out_idx': (j1, j2),
            })
        elif tag == 'replace':
            exp_segment = expected_words[i1:i2]
            out_segment = output_words[j1:j2]
            exp_text = ' '.join(w['text'] for w in exp_segment)
            out_text = ' '.join(w['text'] for w in out_segment)
            exp_start = exp_segment[0].get('start', 0) if exp_segment else 0
            exp_end = exp_segment[-1].get('end', 0) if exp_segment else 0
            out_start = out_segment[0].get('start', 0) if out_segment else 0
            out_end = out_segment[-1].get('end', 0) if out_segment else 0
            replacements.append({
                'exp_text': exp_text,
                'out_text': out_text,
                'exp_word_count': i2 - i1,
                'out_word_count': j2 - j1,
                'exp_start': exp_start,
                'exp_end': exp_end,
                'out_start': out_start,
                'out_end': out_end,
                'exp_idx': (i1, i2),
                'out_idx': (j1, j2),
            })
    
    # Classify replacements as ASR noise vs actual content differences
    asr_noise = []
    content_diffs = []
    
    for r in replacements:
        # If it's a 1-word replacement and words are similar, it's likely ASR noise
        if r['exp_word_count'] == 1 and r['out_word_count'] == 1:
            exp_w = r['exp_text'].lower().strip('.,!?;:')
            out_w = r['out_text'].lower().strip('.,!?;:')
            ratio = difflib.SequenceMatcher(None, exp_w, out_w).ratio()
            if ratio > 0.6:
                asr_noise.append(r)
                continue
        # Small replacements with similar text
        if r['exp_word_count'] <= 3 and r['out_word_count'] <= 3:
            exp_w = r['exp_text'].lower()
            out_w = r['out_text'].lower()
            ratio = difflib.SequenceMatcher(None, exp_w, out_w).ratio()
            if ratio > 0.5:
                asr_noise.append(r)
                continue
        content_diffs.append(r)
    
    # Significant differences (more than 3 words)
    sig_missing = [m for m in missing_from_output if m['word_count'] > 3]
    sig_extra = [e for e in extra_in_output if e['word_count'] > 3]
    sig_replace = [r for r in content_diffs if r['exp_word_count'] > 3 or r['out_word_count'] > 3]
    
    # Calculate content accuracy
    total_missing_words = sum(m['word_count'] for m in missing_from_output)
    total_extra_words = sum(e['word_count'] for e in extra_in_output)
    total_replaced_exp = sum(r['exp_word_count'] for r in replacements)
    total_replaced_out = sum(r['out_word_count'] for r in replacements)
    total_asr_exp = sum(r['exp_word_count'] for r in asr_noise)
    total_content_exp = sum(r['exp_word_count'] for r in content_diffs)
    
    # Generate report
    os.makedirs('reports', exist_ok=True)
    
    with open('reports/transcript_comparison.md', 'w') as f:
        f.write("# Transcript Comparison Report\n\n")
        f.write("## Summary\n\n")
        f.write(f"| Metric | Value |\n")
        f.write(f"|--------|-------|\n")
        f.write(f"| Expected word count | {len(exp_texts)} |\n")
        f.write(f"| Output word count | {len(out_texts)} |\n")
        f.write(f"| Matching words | {total_matching} |\n")
        f.write(f"| Match % (of expected) | {match_pct_exp:.1f}% |\n")
        f.write(f"| Match % (of output) | {match_pct_out:.1f}% |\n")
        f.write(f"| Words missing from output | {total_missing_words} |\n")
        f.write(f"| Extra words in output | {total_extra_words} |\n")
        f.write(f"| Words in replacements (expected side) | {total_replaced_exp} |\n")
        f.write(f"| Words in replacements (output side) | {total_replaced_out} |\n")
        f.write(f"| ASR noise replacements | {len(asr_noise)} ({total_asr_exp} exp words) |\n")
        f.write(f"| Content difference replacements | {len(content_diffs)} ({total_content_exp} exp words) |\n")
        f.write(f"\n")
        
        # Overall assessment
        content_error_words = total_missing_words + total_extra_words + total_content_exp
        content_error_pct = (content_error_words / len(exp_texts) * 100) if exp_texts else 0
        asr_error_pct = (total_asr_exp / len(exp_texts) * 100) if exp_texts else 0
        
        f.write(f"### Overall Assessment\n\n")
        f.write(f"- **Content error rate**: {content_error_pct:.1f}% ({content_error_words} words)\n")
        f.write(f"- **ASR noise rate**: {asr_error_pct:.1f}% ({total_asr_exp} words)\n")
        f.write(f"- **True content match**: {100 - content_error_pct:.1f}%\n\n")
        
        if content_error_pct <= 5:
            f.write(f"✅ **Content match is GOOD** (< 5% error). Differences are primarily ASR transcription noise.\n\n")
        else:
            f.write(f"❌ **Content match needs attention** (> 5% error). Significant content differences found.\n\n")
        
        # Significant missing content
        f.write(f"## Missing Content (in expected, not in output)\n\n")
        f.write(f"Total: {len(missing_from_output)} gaps, {total_missing_words} words\n\n")
        if sig_missing:
            f.write(f"### Significant gaps (>3 words)\n\n")
            for i, m in enumerate(sig_missing):
                f.write(f"**Gap {i+1}** ({m['word_count']} words) at expected timestamp {format_time(m['exp_start'])}-{format_time(m['exp_end'])}:\n")
                text = m['text']
                if len(text) > 300:
                    text = text[:300] + "..."
                f.write(f"> {text}\n\n")
        else:
            f.write(f"No significant gaps found (all gaps ≤ 3 words — likely ASR noise).\n\n")
        
        # Significant extra content
        f.write(f"## Extra Content (in output, not in expected)\n\n")
        f.write(f"Total: {len(extra_in_output)} insertions, {total_extra_words} words\n\n")
        if sig_extra:
            f.write(f"### Significant insertions (>3 words)\n\n")
            for i, e in enumerate(sig_extra):
                f.write(f"**Insert {i+1}** ({e['word_count']} words) at output timestamp {format_time(e['out_start'])}-{format_time(e['out_end'])}:\n")
                text = e['text']
                if len(text) > 300:
                    text = text[:300] + "..."
                f.write(f"> {text}\n\n")
        else:
            f.write(f"No significant insertions found (all insertions ≤ 3 words — likely ASR noise).\n\n")
        
        # Significant content replacements
        f.write(f"## Content Differences (replacements)\n\n")
        f.write(f"Total replacements: {len(replacements)}\n")
        f.write(f"- ASR noise (similar words): {len(asr_noise)}\n")
        f.write(f"- Content differences: {len(content_diffs)}\n\n")
        
        if sig_replace:
            f.write(f"### Significant content differences (>3 words)\n\n")
            for i, r in enumerate(sig_replace):
                f.write(f"**Diff {i+1}** (exp: {r['exp_word_count']} words, out: {r['out_word_count']} words)\n")
                f.write(f"- Expected timestamp: {format_time(r['exp_start'])}-{format_time(r['exp_end'])}\n")
                f.write(f"- Output timestamp: {format_time(r['out_start'])}-{format_time(r['out_end'])}\n")
                exp_text = r['exp_text']
                out_text = r['out_text']
                if len(exp_text) > 300:
                    exp_text = exp_text[:300] + "..."
                if len(out_text) > 300:
                    out_text = out_text[:300] + "..."
                f.write(f"- Expected: > {exp_text}\n")
                f.write(f"- Output: > {out_text}\n\n")
        
        # ASR noise examples
        f.write(f"## ASR Noise Examples (first 20)\n\n")
        for r in asr_noise[:20]:
            f.write(f"- `{r['exp_text']}` → `{r['out_text']}` (exp {format_time(r['exp_start'])})\n")
        f.write(f"\n")
        
        # Timeline analysis
        f.write(f"## Timeline Analysis\n\n")
        f.write(f"Checking for large time gaps or out-of-order content...\n\n")
        
        # Check if output words are in order time-wise
        out_times = [w.get('start', 0) for w in output_words if w.get('start')]
        time_jumps = []
        for i in range(1, len(out_times)):
            diff = out_times[i] - out_times[i-1]
            if diff < -1000:  # Backward jump > 1 second
                time_jumps.append((i, out_times[i-1], out_times[i], diff))
        
        if time_jumps:
            f.write(f"⚠️ Found {len(time_jumps)} backward time jumps in output:\n\n")
            for idx, prev, curr, diff in time_jumps[:10]:
                f.write(f"- At word {idx}: {format_time(prev)} → {format_time(curr)} (jump: {diff/1000:.1f}s)\n")
        else:
            f.write(f"✅ No backward time jumps — content appears to be in correct order.\n\n")
        
        # Summary of problem areas by timestamp
        f.write(f"## Problem Areas by Timestamp\n\n")
        all_problems = []
        for m in sig_missing:
            all_problems.append(('MISSING', m['exp_start'], m['exp_end'], m['word_count'], m['text'][:100]))
        for e in sig_extra:
            all_problems.append(('EXTRA', e['out_start'], e['out_end'], e['word_count'], e['text'][:100]))
        for r in sig_replace:
            all_problems.append(('DIFF', r['exp_start'], r['exp_end'], 
                               r['exp_word_count'] + r['out_word_count'],
                               f"EXP: {r['exp_text'][:50]} → OUT: {r['out_text'][:50]}"))
        
        all_problems.sort(key=lambda x: x[1])
        
        if all_problems:
            f.write(f"| Type | Time | Words | Preview |\n")
            f.write(f"|------|------|-------|---------|\n")
            for ptype, start, end, wc, preview in all_problems:
                f.write(f"| {ptype} | {format_time(start)}-{format_time(end)} | {wc} | {preview[:80]} |\n")
        else:
            f.write(f"No significant problem areas found.\n")
        
        f.write(f"\n## Root Cause Analysis\n\n")
        if content_error_pct <= 5:
            f.write(f"The {content_error_pct:.1f}% content error rate is within acceptable bounds. ")
            f.write(f"The differences are primarily due to:\n\n")
            f.write(f"1. **ASR transcription variability**: Different audio encoding/quality leads to ")
            f.write(f"slightly different word recognition ({len(asr_noise)} instances)\n")
            f.write(f"2. **Minor word boundary differences**: Small insertions/deletions at phrase boundaries\n")
            if sig_missing or sig_extra:
                f.write(f"3. **Small content variations**: {len(sig_missing)} missing segments and {len(sig_extra)} extra segments, ")
                f.write(f"which may represent minor timing differences in the cut points\n")
        else:
            f.write(f"The {content_error_pct:.1f}% content error rate exceeds the 5% threshold. ")
            f.write(f"Investigation needed:\n\n")
            if sig_missing:
                f.write(f"1. **Missing content**: {len(sig_missing)} significant gaps — segments from the expected ")
                f.write(f"output are not present in the actual output. This suggests the alignment algorithm ")
                f.write(f"is cutting segments that should be kept.\n")
            if sig_extra:
                f.write(f"2. **Extra content**: {len(sig_extra)} significant insertions — segments in the output ")
                f.write(f"that shouldn't be there. This suggests bad takes are not being properly removed.\n")
            if sig_replace:
                f.write(f"3. **Content divergence**: {len(sig_replace)} areas where output diverges significantly ")
                f.write(f"from expected — may indicate wrong takes being selected.\n")
    
    print(f"\nReport saved to reports/transcript_comparison.md")
    print(f"\n=== QUICK SUMMARY ===")
    print(f"Match: {match_pct_exp:.1f}% of expected words found in output")
    print(f"Missing: {total_missing_words} words ({len(sig_missing)} significant gaps)")
    print(f"Extra: {total_extra_words} words ({len(sig_extra)} significant insertions)")
    print(f"ASR noise: {len(asr_noise)} replacements")
    print(f"Content diffs: {len(content_diffs)} replacements ({len(sig_replace)} significant)")
    print(f"Content error rate: {content_error_pct:.1f}%")
    
    # Save detailed data for further analysis
    with open('reports/comparison_data.json', 'w') as f:
        json.dump({
            'summary': {
                'expected_words': len(exp_texts),
                'output_words': len(out_texts),
                'matching_words': total_matching,
                'match_pct_expected': match_pct_exp,
                'match_pct_output': match_pct_out,
                'missing_word_count': total_missing_words,
                'extra_word_count': total_extra_words,
                'asr_noise_count': len(asr_noise),
                'content_diff_count': len(content_diffs),
                'content_error_pct': content_error_pct,
            },
            'significant_missing': sig_missing,
            'significant_extra': sig_extra,
            'significant_replacements': [{
                'exp_text': r['exp_text'],
                'out_text': r['out_text'],
                'exp_start': r['exp_start'],
                'exp_end': r['exp_end'],
                'out_start': r['out_start'],
                'out_end': r['out_end'],
            } for r in sig_replace],
        }, f, indent=2, ensure_ascii=False)

if __name__ == "__main__":
    main()
