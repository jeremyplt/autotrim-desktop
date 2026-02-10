#!/usr/bin/env python3
"""Retake Detection FINAL - For porting to Rust."""
import json, re, difflib
from collections import defaultdict

def load_data():
    with open('raw_transcription.json') as f: raw = json.load(f)
    with open('expected_transcription.json') as f: exp = json.load(f)
    return raw['words'], exp['words'], raw.get('text', ''), exp.get('text', '')

def norm(s): return re.sub(r'[^a-z0-9àâäéèêëïîôùûüçœæ]', '', s.lower())
STOP = set(['le','la','les','un','une','des','de','du','au','aux','ce','cette','ces','mon','ma','mes','ton','ta','tes','son','sa','ses','notre','votre','leur','leurs','je','tu','il','elle','on','nous','vous','ils','elles','me','te','se','lui','en','y','ca','et','ou','mais','donc','car','ni','que','qui','quoi','ne','pas','plus','tres','aussi','tout','comme','est','a','sont','ont','fait','va','etre','avoir','jai','dans','sur','avec','pour','par','sans','chez','si','quand','cest','il','ya','bon','oui','non','puis','encore','deja','peu','petit','ici','la','moi','toi','soi','eux','voila','hein','bah','ben','ouais'])
def cw(text): return set(w for w in (norm(w) for w in text.split()) if len(w) >= 3 and w not in STOP)
def opener(text):
    words = [norm(w) for w in text.split()[:3]]
    return tuple(words) if len(words) == 3 else None

def chunk_words(words, gap_ms=500):
    chunks, cur = [], []
    for i, w in enumerate(words):
        cur.append(w)
        g = words[i+1]['start'] - w['end'] if i+1 < len(words) else 99999
        if g >= gap_ms and cur:
            chunks.append({'id': len(chunks), 'text': ' '.join(x['text'] for x in cur),
                'start': cur[0]['start']/1000, 'end': cur[-1]['end']/1000,
                'word_count': len(cur), 'words': cur[:]})
            cur = []
    if cur:
        chunks.append({'id': len(chunks), 'text': ' '.join(x['text'] for x in cur),
            'start': cur[0]['start']/1000, 'end': cur[-1]['end']/1000,
            'word_count': len(cur), 'words': cur[:]})
    return chunks

def gt(chunks, rw, ew):
    rt = [norm(w['text']) for w in rw]; et = [norm(w['text']) for w in ew]
    sm = difflib.SequenceMatcher(None, rt, et, autojunk=False)
    kept = set()
    for b in sm.get_matching_blocks():
        if b.size >= 2:
            for i in range(b.a, b.a+b.size): kept.add(i)
    lk = {}
    for idx, w in enumerate(rw):
        k = (w['start'], w['text'])
        if k not in lk: lk[k] = idx
    labels = []
    for c in chunks:
        ix = [lk.get((w['start'], w['text'])) for w in c['words']]
        ix = [i for i in ix if i is not None]
        if not ix: labels.append(False); continue
        labels.append(sum(1 for i in ix if i in kept)/len(ix) > 0.5)
    return labels

def compare(chunks, keep_ids, exp_text):
    kept_w = [norm(w['text']) for c in chunks if c['id'] in keep_ids for w in c['words']]
    exp_w = [norm(w) for w in exp_text.split()]
    kept_w = [w for w in kept_w if w]; exp_w = [w for w in exp_w if w]
    sm = difflib.SequenceMatcher(None, kept_w, exp_w, autojunk=False)
    m = sum(b.size for b in sm.get_matching_blocks())
    p = m/len(kept_w) if kept_w else 0; r = m/len(exp_w) if exp_w else 0
    return {'p': p, 'r': r, 'f1': 2*p*r/(p+r) if (p+r) > 0 else 0}

def detect(chunks):
    n = len(chunks)
    rm = set()
    pairs = []
    
    # Opener frequency
    ofreq = defaultdict(int)
    for c in chunks:
        o = opener(c['text'])
        if o: ofreq[o] += 1
    
    # ═══ S1: Opener groups with per-member keeper verification ═══
    ogrp = defaultdict(list)
    for c in chunks:
        o = opener(c['text'])
        if o and c['word_count'] >= 3: ogrp[o].append(c['id'])
    
    for op, ids in ogrp.items():
        if len(ids) < 2: continue
        freq = ofreq[op]
        
        # Split by time (max 120s gap)
        subs = [[ids[0]]]
        for k in range(1, len(ids)):
            if chunks[ids[k]]['start'] - chunks[ids[k-1]]['end'] > 120:
                subs.append([ids[k]])
            else: subs[-1].append(ids[k])
        
        for sub in subs:
            if len(sub) < 2: continue
            
            # Group-level overlap check: at least one pair shares content
            min_grp_shared = 3 if freq >= 4 else 2
            has_grp_overlap = False
            for a in range(len(sub)):
                for b in range(a+1, len(sub)):
                    s = cw(chunks[sub[a]]['text']) & cw(chunks[sub[b]]['text'])
                    if len(s) >= min_grp_shared:
                        has_grp_overlap = True; break
                if has_grp_overlap: break
            if not has_grp_overlap: continue
            
            keep_id = sub[-1]
            cw_k = cw(chunks[keep_id]['text'])
            
            # Per-member verification against keeper
            for cid in sub[:-1]:
                cw_c = cw(chunks[cid]['text'])
                shared = cw_c & cw_k
                
                # Basic requirement: share at least 2 content words with keeper
                if len(shared) < 2: continue
                
                # For common openers, require higher overlap
                if freq >= 4:
                    cov_c = len(shared)/len(cw_c) if cw_c else 0
                    cov_k = len(shared)/len(cw_k) if cw_k else 0
                    # Both must have ≥15% coverage AND share ≥4 words
                    if len(shared) < 4 or min(cov_c, cov_k) < 0.15:
                        continue
                
                rm.add(cid)
                pairs.append((cid, keep_id))
    
    # ═══ S2: Zone filling between retake pairs ═══
    for removed_id, keeper_id in list(pairs):
        for bid in range(removed_id + 1, keeper_id):
            if bid in rm: continue
            c = chunks[bid]
            gap = c['start'] - chunks[bid-1]['end']
            low = c['text'] and c['text'][0].islower()
            trunc = c['text'].rstrip().endswith(('—','--','...','…'))
            pr = chunks[bid-1]['id'] in rm
            wc = c['word_count']
            
            do = False
            if wc < 8 and pr and gap < 10: do = True
            if low and pr and gap < 5 and wc < 20: do = True
            if trunc and wc < 12 and pr: do = True
            if wc < 5 and pr: do = True
            
            # Content overlap with keeper
            if not do and wc >= 5:
                ci = cw(c['text']); ck = cw(chunks[keeper_id]['text'])
                sh = ci & ck
                if ci and len(sh)/len(ci) >= 0.25 and len(sh) >= 3: do = True
            
            if do:
                rm.add(bid); pairs.append((bid, keeper_id))
    
    # ═══ S3: High-similarity content detection (very strict) ═══
    for i in range(n):
        if i in rm or chunks[i]['word_count'] < 10: continue
        ci = cw(chunks[i]['text'])
        if len(ci) < 5: continue
        for j in range(i+1, n):
            if j in rm or chunks[j]['word_count'] < 10: continue
            gap = chunks[j]['start'] - chunks[i]['end']
            if gap > 60: break
            if gap < 0: continue
            cj = cw(chunks[j]['text'])
            sh = ci & cj; un = ci | cj
            if len(sh) >= 6 and un and len(sh)/len(un) >= 0.35:
                rm.add(i); pairs.append((i, j)); break
    
    # ═══ S4: Fragment/continuation cleanup ═══
    for _ in range(5):
        changed = False
        for i in range(n):
            if i in rm: continue
            c = chunks[i]
            pr = i > 0 and i-1 in rm
            nr = i < n-1 and i+1 in rm
            gb = c['start'] - chunks[i-1]['end'] if i > 0 else 999
            low = c['text'] and c['text'][0].islower()
            trunc = c['text'].rstrip().endswith(('—','--','...','…'))
            wc = c['word_count']
            
            do = False
            if wc < 5 and pr and nr: do = True
            if wc < 4 and pr and gb < 5: do = True
            if trunc and wc < 8 and (pr or nr): do = True
            if low and wc < 12 and pr and gb < 5: do = True
            
            # Short chunk with most content in nearby later chunk
            if not do and 3 <= wc <= 12:
                ci = cw(c['text'])
                if ci:
                    for j in range(i+1, min(i+5, n)):
                        if chunks[j]['start'] - c['end'] > 30: break
                        sh = ci & cw(chunks[j]['text'])
                        if ci and len(sh)/len(ci) >= 0.6 and chunks[j]['word_count'] > wc:
                            do = True; break
            
            if do: rm.add(i); changed = True
        if not changed: break
    
    # ═══ S5: Non-French ═══
    fr = set(['le','la','les','un','une','des','de','du','je','tu','il','elle','on',
        'nous','vous','et','ou','mais','donc','est','sont','pas','plus','dans','sur',
        'avec','pour','que','qui','ça','ce','cette'])
    for c in chunks:
        if c['id'] in rm: continue
        wl = [w.lower().strip('.,!?') for w in c['text'].split()]
        if wl and sum(1 for w in wl if w in fr)/len(wl) < 0.1 and c['word_count'] >= 3:
            rm.add(c['id'])
    
    return set(c['id'] for c in chunks) - rm

def main():
    rw, ew, rt, et = load_data()
    ch = chunk_words(rw, 500)
    labels = gt(ch, rw, ew)
    print(f"Chunks: {len(ch)}, GT: {sum(labels)} keep / {len(labels)-sum(labels)} remove")
    
    keep = detect(ch)
    
    gk = set(i for i,l in enumerate(labels) if l)
    gr = set(i for i,l in enumerate(labels) if not l)
    ar = set(c['id'] for c in ch) - keep
    ck = keep & gk; cr = ar & gr; fp = ar & gk; fn = keep & gr
    acc = (len(ck)+len(cr))/len(ch)
    m = compare(ch, keep, et)
    
    print(f"\nAlgo: keep {len(keep)}, remove {len(ar)} | GT: keep {len(gk)}, remove {len(gr)}")
    print(f"Accuracy: {acc*100:.1f}% | FP: {len(fp)} FN: {len(fn)}")
    print(f"Removal precision: {len(cr)}/{len(ar)} = {len(cr)/len(ar)*100:.0f}%")
    print(f"Text: P={m['p']*100:.1f}% R={m['r']*100:.1f}% F1={m['f1']*100:.1f}%")
    
    if fp:
        print(f"\nFP ({len(fp)}):")
        for cid in sorted(fp):
            print(f"  [{cid}] {ch[cid]['start']:.0f}s ({ch[cid]['word_count']}w): {ch[cid]['text'][:70]}")
    print(f"\nFN ({len(fn)}, showing first 20):")
    for cid in sorted(fn)[:20]:
        print(f"  [{cid}] {ch[cid]['start']:.0f}s ({ch[cid]['word_count']}w): {ch[cid]['text'][:70]}")

if __name__ == '__main__': main()
