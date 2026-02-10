#!/usr/bin/env python3
"""
Build a precise ground truth mapping between raw and expected transcriptions.
Figure out which raw words/time ranges map to which expected words.
"""
import json
import difflib
import re

def normalize(s):
    return re.sub(r'[^a-z0-9àâäéèêëïîôùûüçœæ]', '', s.lower())

with open('raw_transcription.json') as f:
    raw = json.load(f)
with open('expected_transcription.json') as f:
    exp = json.load(f)

raw_words = raw['words']
exp_words = exp['words']

# Align raw and expected words using SequenceMatcher
raw_texts = [normalize(w['text']) for w in raw_words]
exp_texts = [normalize(w['text']) for w in exp_words]

print("Computing alignment...")
sm = difflib.SequenceMatcher(None, raw_texts, exp_texts, autojunk=False)
matching_blocks = sm.get_matching_blocks()

# Mark which raw word indices are "kept"
kept_raw_indices = set()
for block in matching_blocks:
    if block.size >= 2:
        for i in range(block.a, block.a + block.size):
            kept_raw_indices.add(i)

print(f"Matched {len(kept_raw_indices)}/{len(raw_words)} raw words")

# Find contiguous "kept" and "removed" ranges in raw
ranges = []  # (type, start_idx, end_idx, start_time, end_time, text_preview)
current_type = None
range_start = 0

for i in range(len(raw_words)):
    is_kept = i in kept_raw_indices
    new_type = 'KEEP' if is_kept else 'REMOVE'
    
    if new_type != current_type:
        if current_type is not None:
            ranges.append((current_type, range_start, i-1, 
                          raw_words[range_start]['start']/1000, 
                          raw_words[i-1]['end']/1000))
        current_type = new_type
        range_start = i

if current_type is not None:
    ranges.append((current_type, range_start, len(raw_words)-1,
                  raw_words[range_start]['start']/1000,
                  raw_words[-1]['end']/1000))

# Merge small gaps (< 3 words) in KEEP ranges
# Sometimes a word is missed in alignment but the surrounding area is kept
merged = []
for r in ranges:
    if merged and r[0] == merged[-1][0]:
        # Same type, merge
        merged[-1] = (r[0], merged[-1][1], r[2], merged[-1][3], r[4])
    elif merged and r[0] == 'REMOVE' and (r[2] - r[1]) < 3:
        # Tiny remove gap - check if surrounded by keeps
        if len(merged) >= 1 and merged[-1][0] == 'KEEP':
            # Merge into previous keep
            merged[-1] = ('KEEP', merged[-1][1], r[2], merged[-1][3], r[4])
    else:
        merged.append(r)

# Re-merge adjacent same-type
final = [merged[0]]
for r in merged[1:]:
    if r[0] == final[-1][0]:
        final[-1] = (r[0], final[-1][1], r[2], final[-1][3], r[4])
    else:
        final.append(r)

# Now build chunks
chunks = []
current = []
for i, w in enumerate(raw_words):
    current.append(w)
    gap = raw_words[i+1]['start'] - w['end'] if i+1 < len(raw_words) else 99999
    if gap >= 500 and current:
        text = ' '.join(ww['text'] for ww in current)
        chunks.append({
            'id': len(chunks),
            'text': text,
            'start': current[0]['start'] / 1000,
            'end': current[-1]['end'] / 1000,
            'word_count': len(current),
            'words': current[:],
        })
        current = []
if current:
    text = ' '.join(ww['text'] for ww in current)
    chunks.append({
        'id': len(chunks),
        'text': text,
        'start': current[0]['start'] / 1000,
        'end': current[-1]['end'] / 1000,
        'word_count': len(current),
        'words': current[:],
    })

# Print the keep/remove ranges with their time ranges
print(f"\n{'='*80}")
print(f"KEEP/REMOVE RANGES (merged)")
print(f"{'='*80}")
total_keep_time = 0
total_remove_time = 0
for r_type, r_start, r_end, t_start, t_end in final:
    word_count = r_end - r_start + 1
    duration = t_end - t_start
    preview = ' '.join(raw_words[i]['text'] for i in range(r_start, min(r_start + 12, r_end + 1)))
    
    if r_type == 'KEEP':
        total_keep_time += duration
    else:
        total_remove_time += duration
    
    print(f"  {r_type:6s} {t_start:7.1f}-{t_end:7.1f}s ({duration:5.1f}s, {word_count:4d}w): {preview}...")

print(f"\nTotal keep time: {total_keep_time:.0f}s = {total_keep_time/60:.1f}min")
print(f"Total remove time: {total_remove_time:.0f}s = {total_remove_time/60:.1f}min")

# For each chunk, determine if it's keep or remove
raw_word_lookup = {}
for idx, w in enumerate(raw_words):
    key = (w['start'], w['text'])
    if key not in raw_word_lookup:
        raw_word_lookup[key] = idx

print(f"\n{'='*80}")
print(f"CHUNK LABELS")
print(f"{'='*80}")

# Group consecutive remove chunks to identify retake areas
remove_areas = []
current_area = []

for chunk in chunks:
    indices = [raw_word_lookup.get((w['start'], w['text'])) for w in chunk['words']]
    indices = [i for i in indices if i is not None]
    if not indices:
        label = 'REMOVE'
    else:
        kept_count = sum(1 for idx in indices if idx in kept_raw_indices)
        keep_ratio = kept_count / len(indices)
        label = 'KEEP' if keep_ratio > 0.5 else 'REMOVE'
    
    chunk['label'] = label
    
    if label == 'REMOVE':
        if not current_area or chunk['start'] - current_area[-1]['end'] < 30:
            current_area.append(chunk)
        else:
            if current_area:
                remove_areas.append(current_area)
            current_area = [chunk]
    else:
        if current_area:
            remove_areas.append(current_area)
            current_area = []

if current_area:
    remove_areas.append(current_area)

# Print remove areas with context
print(f"\nFound {len(remove_areas)} remove areas:")
for area in remove_areas:
    first = area[0]
    last = area[-1]
    total_words = sum(c['word_count'] for c in area)
    
    # Find kept chunks before and after
    before = None
    after = None
    for c in chunks:
        if c['end'] <= first['start'] and c['label'] == 'KEEP':
            before = c
        if c['start'] >= last['end'] and c['label'] == 'KEEP' and after is None:
            after = c
    
    print(f"\n  REMOVE AREA: {first['start']:.1f}-{last['end']:.1f}s ({len(area)} chunks, {total_words}w)")
    if before:
        print(f"    ← BEFORE (KEEP): [{before['id']}] {before['start']:.1f}-{before['end']:.1f}s: {before['text'][:60]}...")
    for c in area:
        print(f"    REMOVE [{c['id']}] {c['start']:.1f}-{c['end']:.1f}s ({c['word_count']}w): {c['text'][:60]}...")
    if after:
        print(f"    → AFTER (KEEP): [{after['id']}] {after['start']:.1f}-{after['end']:.1f}s: {after['text'][:60]}...")

# Save ground truth
gt = {chunk['id']: chunk['label'] for chunk in chunks}
with open('reports/ground_truth.json', 'w') as f:
    json.dump(gt, f, indent=2)
print(f"\nSaved ground truth to reports/ground_truth.json")
