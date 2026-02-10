#!/usr/bin/env python3
"""
Retake detection test — Phase 1 algorithmic detection.
Goal: maximize true removals while keeping FP ≤ 5.
Then Phase 2 (Claude) handles the rest.
"""
import json, re, difflib, sys, unicodedata
from collections import defaultdict

# ──────────────────────────────────────────────────────────
# Utilities  
# ──────────────────────────────────────────────────────────

def norm(s):
    s = unicodedata.normalize('NFC', s.lower())
    accent_map = str.maketrans('àâäéèêëïîôöùûüçœæ', 'aaaeeeeiioouuucoa')
    s = s.translate(accent_map)
    s = unicodedata.normalize('NFKD', s)
    s = ''.join(c for c in s if not unicodedata.combining(c))
    return re.sub(r'[^a-z0-9]', '', s)

STOP = set([
    'le','la','les','un','une','des','de','du','au','aux',
    'ce','cette','ces','mon','ma','mes','ton','ta','tes',
    'son','sa','ses','notre','votre','leur','leurs',
    'je','tu','il','elle','on','nous','vous','ils','elles',
    'me','te','se','lui','en','y','ca',
    'et','ou','mais','donc','car','ni','que','qui','quoi',
    'ne','pas','plus','tres','aussi','tout','comme',
    'est','a','sont','ont','fait','va','etre','avoir','jai',
    'dans','sur','avec','pour','par','sans','chez',
    'si','quand','cest','il','ya','bon','oui','non',
    'puis','encore','deja','peu','petit','ici','la',
    'moi','toi','soi','eux',
    'voila','hein','bah','ben','ouais',
])

def cw(text):
    return set(w for w in (norm(w) for w in text.split()) if len(w) >= 3 and w not in STOP)

def opener(text, n=3):
    words = [norm(w) for w in text.split()[:n]]
    return tuple(words) if len(words) == n else None

def is_truncated(text):
    t = text.rstrip()
    return t.endswith(('—','--','...','…'))

# ──────────────────────────────────────────────────────────
# Data loading
# ──────────────────────────────────────────────────────────

def load_data():
    with open('raw_transcription.json') as f: raw = json.load(f)
    with open('expected_transcription.json') as f: exp = json.load(f)
    return raw['words'], exp['words'], raw.get('text', ''), exp.get('text', '')

def chunk_words(words, gap_ms=500):
    chunks, cur = [], []
    for i, w in enumerate(words):
        cur.append(w)
        g = words[i+1]['start'] - w['end'] if i+1 < len(words) else 99999
        if g >= 500 and cur:
            chunks.append({'id': len(chunks), 'text': ' '.join(x['text'] for x in cur),
                'start': cur[0]['start']/1000, 'end': cur[-1]['end']/1000,
                'word_count': len(cur), 'words': cur[:]})
            cur = []
    if cur:
        chunks.append({'id': len(chunks), 'text': ' '.join(x['text'] for x in cur),
            'start': cur[0]['start']/1000, 'end': cur[-1]['end']/1000,
            'word_count': len(cur), 'words': cur[:]})
    return chunks

def ground_truth(chunks, raw_words, exp_words):
    rt = [norm(w['text']) for w in raw_words]
    et = [norm(w['text']) for w in exp_words]
    sm = difflib.SequenceMatcher(None, rt, et, autojunk=False)
    kept_indices = set()
    for b in sm.get_matching_blocks():
        if b.size >= 2:
            for i in range(b.a, b.a + b.size): kept_indices.add(i)
    word_to_idx = {}
    for idx, w in enumerate(raw_words):
        k = (w['start'], w['text'])
        if k not in word_to_idx: word_to_idx[k] = idx
    labels = []
    for c in chunks:
        indices = [word_to_idx.get((w['start'], w['text'])) for w in c['words']]
        indices = [i for i in indices if i is not None]
        if not indices: labels.append(False); continue
        labels.append(sum(1 for i in indices if i in kept_indices) / len(indices) > 0.5)
    return labels

# ──────────────────────────────────────────────────────────
# Detection Algorithm
# ──────────────────────────────────────────────────────────

def detect(chunks, verbose=False):
    n = len(chunks)
    rm = set()
    pairs = []
    cw_cache = {c['id']: cw(c['text']) for c in chunks}
    
    ofreq = defaultdict(int)
    for c in chunks:
        o = opener(c['text'])
        if o: ofreq[o] += 1
    
    # ═══ S1: Opener groups (3-word) ═══
    ogrp = defaultdict(list)
    for c in chunks:
        o = opener(c['text'])
        if o and c['word_count'] >= 3:
            ogrp[o].append(c['id'])
    
    for op, ids in ogrp.items():
        if len(ids) < 2: continue
        freq = ofreq[op]
        
        subs = [[ids[0]]]
        for k in range(1, len(ids)):
            if chunks[ids[k]]['start'] - chunks[ids[k-1]]['end'] > 120:
                subs.append([ids[k]])
            else:
                subs[-1].append(ids[k])
        
        for sub in subs:
            if len(sub) < 2: continue
            
            min_grp_shared = 3 if freq >= 4 else 2
            has_grp_overlap = False
            for a in range(len(sub)):
                for b in range(a+1, len(sub)):
                    s = cw_cache[sub[a]] & cw_cache[sub[b]]
                    if len(s) >= min_grp_shared:
                        has_grp_overlap = True; break
                if has_grp_overlap: break
            if not has_grp_overlap: continue
            
            keep_id = sub[-1]
            cw_k = cw_cache[keep_id]
            
            # Check if opener is "weak" (mostly stop/short words)
            weak_opener = sum(1 for w in op if w in STOP or len(w) <= 3) >= 2
            
            for cid in sub[:-1]:
                cw_c = cw_cache[cid]
                shared = cw_c & cw_k
                if len(shared) < 2: continue
                
                if freq >= 4 or weak_opener:
                    cov_c = len(shared)/len(cw_c) if cw_c else 0
                    cov_k = len(shared)/len(cw_k) if cw_k else 0
                    
                    if weak_opener:
                        # Weak openers need strong content overlap
                        if len(shared) < 4 or min(cov_c, cov_k) < 0.25:
                            continue
                    elif freq >= 6:
                        if len(shared) < 5 or min(cov_c, cov_k) < 0.20:
                            continue
                    else:
                        if len(shared) < 4 or min(cov_c, cov_k) < 0.15:
                            continue
                
                rm.add(cid)
                pairs.append((cid, keep_id))
                if verbose: print(f"  S1: REMOVE [{cid}] → KEEP [{keep_id}]")
    
    if verbose: print(f"After S1: {len(rm)} removed")
    
    # ═══ S2: Zone filling between retake pairs ═══
    for removed_id, keeper_id in list(pairs):
        for bid in range(removed_id + 1, keeper_id):
            if bid in rm: continue
            c = chunks[bid]
            gap = c['start'] - chunks[bid-1]['end'] if bid > 0 else 999
            pr = (bid-1) in rm if bid > 0 else False
            wc = c['word_count']
            low = c['text'] and c['text'][0].islower()
            trunc = is_truncated(c['text'])
            
            do = False
            if wc < 8 and pr and gap < 10: do = True
            if low and pr and gap < 5 and wc < 20: do = True
            if trunc and wc < 12 and pr: do = True
            if wc < 5 and pr: do = True
            
            if not do and wc >= 5:
                ci = cw_cache[bid]; ck = cw_cache[keeper_id]
                sh = ci & ck
                if ci and len(sh) >= 3 and len(sh)/len(ci) >= 0.25: do = True
            
            if do:
                rm.add(bid)
                pairs.append((bid, keeper_id))
                if verbose: print(f"  S2: REMOVE [{bid}] (zone fill)")
    
    if verbose: print(f"After S2: {len(rm)} removed")
    
    # ═══ S3: High-similarity content detection ═══
    # Conservative: high Jaccard + enough shared words
    for i in range(n):
        if i in rm or chunks[i]['word_count'] < 8: continue
        ci = cw_cache[i]
        if len(ci) < 4: continue
        for j in range(i+1, n):
            if j in rm or chunks[j]['word_count'] < 8: continue
            gap = chunks[j]['start'] - chunks[i]['end']
            if gap > 200: break
            if gap < 0: continue
            cj = cw_cache[j]
            sh = ci & cj
            un = ci | cj
            
            # High Jaccard
            if len(sh) >= 5 and un and len(sh)/len(un) >= 0.35:
                rm.add(i); pairs.append((i, j))
                if verbose: print(f"  S3-J: REMOVE [{i}] → KEEP [{j}] (jac={len(sh)/len(un):.2f})")
                break
            
            # High coverage of shorter, later chunk bigger
            if len(sh) >= 5:
                min_len = min(len(ci), len(cj))
                coverage = len(sh) / min_len if min_len > 0 else 0
                if coverage >= 0.55 and chunks[j]['word_count'] > chunks[i]['word_count']:
                    rm.add(i); pairs.append((i, j))
                    if verbose: print(f"  S3-C: REMOVE [{i}] → KEEP [{j}] (cov={coverage:.2f})")
                    break
    
    if verbose: print(f"After S3: {len(rm)} removed")
    
    # ═══ S4: Fragment cleanup (multi-pass) ═══
    for _pass in range(5):
        changed = False
        for i in range(n):
            if i in rm: continue
            c = chunks[i]
            pr = i > 0 and (i-1) in rm
            nr = i < n-1 and (i+1) in rm
            gb = c['start'] - chunks[i-1]['end'] if i > 0 else 999
            wc = c['word_count']
            low = c['text'] and c['text'][0].islower()
            trunc = is_truncated(c['text'])
            
            do = False
            if wc < 5 and pr and nr: do = True
            if wc < 4 and pr and gb < 5: do = True
            if trunc and wc < 10 and (pr or nr): do = True
            if low and wc < 15 and pr and gb < 5: do = True
            
            # Truncated even without neighbors
            if not do and trunc and wc < 12:
                ci = cw_cache[i]
                if ci:
                    for j in range(i+1, min(i+8, n)):
                        if chunks[j]['start'] - c['end'] > 60: break
                        sh = ci & cw_cache[j]
                        if ci and len(sh)/len(ci) >= 0.5:
                            do = True; break
            
            # Short chunk with content in nearby later chunk
            if not do and 3 <= wc <= 15:
                ci = cw_cache[i]
                if ci:
                    for j in range(i+1, min(i+10, n)):
                        if chunks[j]['start'] - c['end'] > 120: break
                        sh = ci & cw_cache[j]
                        if ci and len(sh) >= 1 and len(sh)/len(ci) >= 0.5 and chunks[j]['word_count'] > wc:
                            do = True; break
            
            if do:
                rm.add(i); changed = True
                if verbose: print(f"  S4: REMOVE [{i}] ({wc}w)")
        if not changed: break
    
    if verbose: print(f"After S4: {len(rm)} removed")
    
    # ═══ S5: Non-French detection ═══
    fr = set(['le','la','les','un','une','des','de','du','je','tu','il','elle','on',
        'nous','vous','et','ou','mais','donc','est','sont','pas','plus','dans','sur',
        'avec','pour','que','qui','ça','ce','cette'])
    for c in chunks:
        if c['id'] in rm: continue
        wl = [w.lower().strip('.,!?') for w in c['text'].split()]
        if wl and sum(1 for w in wl if w in fr)/len(wl) < 0.1 and c['word_count'] >= 3:
            rm.add(c['id'])
            if verbose: print(f"  S5: REMOVE [{c['id']}] (non-French)")
    
    if verbose: print(f"After S5: {len(rm)} removed")
    
    # ═══ S6: Sandwiched/tiny cleanup ═══
    for _pass in range(3):
        changed = False
        for i in range(n):
            if i in rm: continue
            wc = chunks[i]['word_count']
            pr = i > 0 and (i-1) in rm
            nr = i < n-1 and (i+1) in rm
            gb = chunks[i]['start'] - chunks[i-1]['end'] if i > 0 else 999
            ga = chunks[i+1]['start'] - chunks[i]['end'] if i < n-1 else 999
            
            # Sandwiched, small gaps, short
            if pr and nr and gb < 10 and ga < 10 and wc <= 20:
                rm.add(i); changed = True
                if verbose: print(f"  S6: REMOVE [{i}] (sandwiched, {wc}w)")
                continue
            
            # Tiny adjacent to removed
            if wc <= 3 and (pr or nr):
                rm.add(i); changed = True
                if verbose: print(f"  S6: REMOVE [{i}] (tiny, {wc}w)")
                continue
            
            # Short fragment adjacent to removed with small gap
            if wc <= 5 and ((pr and gb < 5) or (nr and ga < 5)):
                rm.add(i); changed = True
                if verbose: print(f"  S6: REMOVE [{i}] (short adj, {wc}w)")
                continue
        if not changed: break
    
    if verbose: print(f"After S6: {len(rm)} removed")
    
    # ═══ S7: Superseded take (conservative) ═══
    for i in range(n):
        if i in rm: continue
        ci = cw_cache[i]
        if len(ci) < 4: continue
        wc_i = chunks[i]['word_count']
        
        for j in range(i+1, n):
            if j in rm: continue
            gap = chunks[j]['start'] - chunks[i]['end']
            if gap > 300: break
            if gap < 0: continue
            cj = cw_cache[j]
            if len(cj) < 4: continue
            wc_j = chunks[j]['word_count']
            
            sh = ci & cj
            coverage_i = len(sh) / len(ci) if ci else 0
            
            if coverage_i >= 0.70 and wc_j >= wc_i * 2.0 and len(sh) >= 4:
                rm.add(i)
                pairs.append((i, j))
                if verbose: print(f"  S7: REMOVE [{i}] ({wc_i}w) → [{j}] ({wc_j}w)")
                break
    
    if verbose: print(f"After S7: {len(rm)} removed")
    
    # ═══ S8: Zone-fill for new pairs ═══
    all_pairs = [(r, k) for r, k in pairs if r in rm and k not in rm]
    for removed_id, keeper_id in all_pairs:
        for bid in range(removed_id + 1, keeper_id):
            if bid in rm: continue
            c = chunks[bid]
            gap = c['start'] - chunks[bid-1]['end'] if bid > 0 else 999
            pr = bid > 0 and (bid-1) in rm
            wc = c['word_count']
            low = c['text'] and c['text'][0].islower()
            trunc = is_truncated(c['text'])
            
            do = False
            if wc < 8 and pr and gap < 10: do = True
            if low and pr and gap < 5 and wc < 20: do = True
            if trunc and wc < 12 and pr: do = True
            if wc < 5 and pr: do = True
            if not do and wc >= 5:
                ci = cw_cache[bid]; ck = cw_cache[keeper_id]
                sh = ci & ck
                # Stricter for longer chunks: they likely have unique content
                min_cov = 0.40 if wc > 20 else 0.25
                min_sh = 4 if wc > 20 else 3
                if ci and len(sh) >= min_sh and len(sh)/len(ci) >= min_cov: do = True
            
            if do:
                rm.add(bid)
                if verbose: print(f"  S8: REMOVE [{bid}] (zone fill)")
    
    if verbose: print(f"After S8: {len(rm)} removed")
    
    # ═══ S9: Extended zone cleanup ═══
    for _pass in range(3):
        changed = False
        for i in range(n):
            if i in rm: continue
            wc = chunks[i]['word_count']
            gb = chunks[i]['start'] - chunks[i-1]['end'] if i > 0 else 999
            
            rm_before = 0
            j = i - 1
            while j >= 0 and j in rm: rm_before += 1; j -= 1
            rm_after = 0
            j = i + 1
            while j < n and j in rm: rm_after += 1; j += 1
            
            # Within large removed zone, short chunk
            if rm_before >= 2 and rm_after >= 2 and wc <= 15:
                rm.add(i); changed = True
                if verbose: print(f"  S9: REMOVE [{i}] (zone, {wc}w)")
                continue
            
            # After long removed run, short, close gap
            if rm_before >= 3 and wc <= 10 and gb < 5:
                rm.add(i); changed = True
                if verbose: print(f"  S9: REMOVE [{i}] (after run, {wc}w)")
                continue
        if not changed: break
    
    if verbose: print(f"After S9: {len(rm)} removed")
    
    # ═══ S10: Orphan cleanup ═══
    for i in range(n):
        if i in rm: continue
        wc = chunks[i]['word_count']
        if wc > 8: continue
        pr = i > 0 and (i-1) in rm
        nr = i < n-1 and (i+1) in rm
        ga = chunks[i+1]['start'] - chunks[i]['end'] if i < n-1 else 999
        gb = chunks[i]['start'] - chunks[i-1]['end'] if i > 0 else 999
        
        if pr and ga > 20 and wc <= 8:
            rm.add(i)
            if verbose: print(f"  S10: REMOVE [{i}] (orphan, {wc}w)")
    
    if verbose: print(f"After S10: {len(rm)} removed")
    
    keep = set(c['id'] for c in chunks) - rm
    return keep


# ──────────────────────────────────────────────────────────
# Evaluation
# ──────────────────────────────────────────────────────────

def evaluate(chunks, keep_ids, labels, exp_text):
    gk = set(i for i,l in enumerate(labels) if l)
    gr = set(i for i,l in enumerate(labels) if not l)
    ar = set(c['id'] for c in chunks) - keep_ids
    ck = keep_ids & gk
    cr = ar & gr
    fp = ar & gk
    fn = keep_ids & gr
    acc = (len(ck)+len(cr))/len(chunks)
    
    kept_dur = sum(c['end'] - c['start'] for c in chunks if c['id'] in keep_ids)
    expected_dur = sum(c['end'] - c['start'] for c in chunks if c['id'] in gk)
    
    kept_w = [norm(w['text']) for c in chunks if c['id'] in keep_ids for w in c['words']]
    exp_w = [norm(w) for w in exp_text.split()]
    kept_w = [w for w in kept_w if w]; exp_w = [w for w in exp_w if w]
    sm = difflib.SequenceMatcher(None, kept_w, exp_w, autojunk=False)
    m = sum(b.size for b in sm.get_matching_blocks())
    p = m/len(kept_w) if kept_w else 0
    r = m/len(exp_w) if exp_w else 0
    f1 = 2*p*r/(p+r) if (p+r) > 0 else 0
    
    return {
        'accuracy': acc, 'fp': sorted(fp), 'fn': sorted(fn),
        'kept_dur': kept_dur, 'expected_dur': expected_dur,
        'precision': p, 'recall': r, 'f1': f1,
        'keep_count': len(keep_ids), 'remove_count': len(ar),
        'gt_keep': len(gk), 'gt_remove': len(gr),
    }


def main():
    verbose = '--verbose' in sys.argv or '-v' in sys.argv
    rw, ew, rt, et = load_data()
    chunks = chunk_words(rw, 500)
    labels = ground_truth(chunks, rw, ew)
    
    print(f"Chunks: {len(chunks)}, GT: {sum(labels)} keep / {len(labels)-sum(labels)} remove\n")
    
    keep = detect(chunks, verbose=verbose)
    m = evaluate(chunks, keep, labels, et)
    
    print(f"\n{'='*60}")
    print(f"Algorithm: keep {m['keep_count']}, remove {m['remove_count']} | GT: keep {m['gt_keep']}, remove {m['gt_remove']}")
    print(f"Accuracy: {m['accuracy']*100:.1f}% | FP: {len(m['fp'])} | FN: {len(m['fn'])}")
    print(f"Output: {m['kept_dur']/60:.1f} min (expected: {m['expected_dur']/60:.1f} min)")
    print(f"Text: P={m['precision']*100:.1f}% R={m['recall']*100:.1f}% F1={m['f1']*100:.1f}%")
    
    if m['fp']:
        print(f"\nFP ({len(m['fp'])}):")
        for cid in m['fp']:
            c = chunks[cid]
            print(f"  [{cid}] {c['start']:.0f}s ({c['word_count']}w): {c['text'][:80]}")
    
    if m['fn']:
        print(f"\nFN ({len(m['fn'])}):")
        for cid in m['fn']:
            c = chunks[cid]
            print(f"  [{cid}] {c['start']:.0f}s ({c['word_count']}w): {c['text'][:80]}")
    
    target = 31.0
    if m['kept_dur']/60 <= target:
        print(f"\n✅ {m['kept_dur']/60:.1f} min ≤ {target} min")
    else:
        print(f"\n❌ {m['kept_dur']/60:.1f} min > {target} min ({m['kept_dur']/60 - target:.1f} min over)")
    return m


if __name__ == '__main__':
    main()
