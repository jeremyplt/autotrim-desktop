#!/usr/bin/env python3
"""
Full AutoTrim Pipeline Test — Phase 1 (algorithmic) + Phase 2 (Claude API)
Compares final output against expected_transcription.json
"""
import json, re, sys, os, difflib, unicodedata, time
from collections import defaultdict

# ──────────────────────────────────────────────────────────
# Utilities (shared with test_pipeline.py)
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
# Phase 1: Algorithmic Detection (improved)
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
                        # IMPROVED: use 0.15 threshold (was 0.20)
                        # Also allow high max_cov with enough shared words
                        if len(shared) < 5:
                            continue
                        if min(cov_c, cov_k) < 0.15:
                            # Fallback: if one side has very high coverage, still match
                            if not (max(cov_c, cov_k) >= 0.40 and len(shared) >= 7):
                                continue
                    else:
                        if len(shared) < 4 or min(cov_c, cov_k) < 0.15:
                            continue
                
                rm.add(cid)
                pairs.append((cid, keep_id))
                if verbose: print(f"  S1: REMOVE [{cid}] → KEEP [{keep_id}]")
    
    if verbose: print(f"After S1: {len(rm)} removed")
    
    # ═══ S2: Zone filling between retake pairs ═══
    s1_keepers = set(k for _, k in pairs if k not in rm)
    for removed_id, keeper_id in list(pairs):
        for bid in range(removed_id + 1, keeper_id):
            if bid in rm: continue
            if bid in s1_keepers: continue  # Don't zone-fill explicit keepers
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
            
            if len(sh) >= 5 and un and len(sh)/len(un) >= 0.35:
                rm.add(i); pairs.append((i, j))
                if verbose: print(f"  S3-J: REMOVE [{i}] → KEEP [{j}]")
                break
            
            if len(sh) >= 5:
                min_len = min(len(ci), len(cj))
                coverage = len(sh) / min_len if min_len > 0 else 0
                if coverage >= 0.55 and chunks[j]['word_count'] > chunks[i]['word_count']:
                    rm.add(i); pairs.append((i, j))
                    if verbose: print(f"  S3-C: REMOVE [{i}] → KEEP [{j}]")
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
            
            if not do and trunc and wc < 12:
                ci = cw_cache[i]
                if ci:
                    for j in range(i+1, min(i+8, n)):
                        if chunks[j]['start'] - c['end'] > 60: break
                        sh = ci & cw_cache[j]
                        if ci and len(sh)/len(ci) >= 0.5:
                            do = True; break
            
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
            
            if pr and nr and gb < 10 and ga < 10 and wc <= 20:
                rm.add(i); changed = True
                if verbose: print(f"  S6: REMOVE [{i}] (sandwiched, {wc}w)")
                continue
            
            if wc <= 3 and (pr or nr):
                rm.add(i); changed = True
                if verbose: print(f"  S6: REMOVE [{i}] (tiny, {wc}w)")
                continue
            
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
    # Protect explicit keepers from prior strategies
    explicit_keepers = set(k for _, k in pairs if k not in rm)
    
    all_pairs = [(r, k) for r, k in pairs if r in rm and k not in rm]
    for removed_id, keeper_id in all_pairs:
        for bid in range(removed_id + 1, keeper_id):
            if bid in rm: continue
            if bid in explicit_keepers: continue  # Don't zone-fill explicit keepers
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
            
            if rm_before >= 2 and rm_after >= 2 and wc <= 15:
                rm.add(i); changed = True
                if verbose: print(f"  S9: REMOVE [{i}] (zone, {wc}w)")
                continue
            
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
    
    # S11 removed — heuristic AI command detection was too imprecise
    
    keep = set(c['id'] for c in chunks) - rm
    return keep


# ──────────────────────────────────────────────────────────
# Phase 2: Claude API Call
# ──────────────────────────────────────────────────────────

def format_time(seconds):
    mins = int(seconds / 60)
    secs = seconds % 60
    return f"{mins}:{secs:05.2f}"

def build_claude_prompt(chunks, keep_ids, mode="moderate"):
    """Build the prompt for Claude Phase 2"""
    surviving = [c for c in chunks if c['id'] in keep_ids]
    
    transcript = ""
    for i, chunk in enumerate(surviving):
        if i > 0:
            prev = surviving[i-1]
            gap = chunk['start'] - prev['end']
            if gap >= 1.0:
                transcript += f"  --- {gap:.1f}s ---\n"
        
        continuation = " ⟵ SUITE" if chunk['text'] and chunk['text'][0].islower() else ""
        transcript += (
            f"[{chunk['id']}] {format_time(chunk['start'])}-{format_time(chunk['end'])} "
            f"({chunk['end']-chunk['start']:.1f}s, {chunk['word_count']} mots){continuation} "
            f"{chunk['text']}\n"
        )
    
    mode_instruction = {
        "aggressive": "Mode agressif : identifie toutes les reprises probables, y compris les cas ambigus.",
        "conservative": "Mode conservateur : identifie UNIQUEMENT les reprises évidentes et indiscutables. Au moindre doute, garde le passage.",
    }.get(mode, "Mode modéré : identifie les reprises claires et probables. En cas de doute léger, garde le passage.")
    
    system_prompt = f"""Tu es un assistant de montage vidéo expert. Tu analyses une transcription PRÉ-NETTOYÉE d'un rush vidéo pour créer un montage final professionnel.

Les reprises évidentes ont DÉJÀ été supprimées automatiquement. Tu vois uniquement les segments survivants. Ton travail est de nettoyer DAVANTAGE.

## TON TRAVAIL
Retourne les IDs des segments à GARDER dans le montage final. Tout ce que tu ne retournes pas sera coupé.

## RÈGLE CRITIQUE #1: SÉLECTION DE VERSION
Quand tu détectes des segments qui couvrent le MÊME sujet/idée (reprises), tu DOIS garder la version la plus TARDIVE chronologiquement (= la dernière dans le temps). 
JAMAIS garder une version antérieure et supprimer la postérieure. Le locuteur fait des reprises pour s'améliorer — la dernière tentative est TOUJOURS la bonne.
Exemple: Si [A] à 300s et [B] à 400s disent la même chose → SUPPRIME [A], GARDE [B]. Même si [A] est plus long ou plus détaillé.

## CE QUE TU DOIS SUPPRIMER

### 1. Reprises (même sujet répété)
Quand le locuteur aborde le MÊME sujet/idée dans plusieurs segments (même éloignés de 5 min), SUPPRIME toutes les versions SAUF la dernière. Indices d'une reprise:
- Même thème ou vocabulaire similaire
- Version antérieure incomplète, hésitante, ou moins fluide
- Phrase qui recommence une idée déjà exprimée plus tard

### 2. Faux départs et fragments abandonnés
- Phrases inachevées ou qui s'interrompent
- Segments très courts (<8 mots) sans pensée complète
- Segments marqués ⟵ SUITE dont le parent est supprimé

### 3. Instructions DÉTAILLÉES dictées à un agent IA
SUPPRIME les passages où le locuteur dicte des instructions techniques détaillées à un agent IA. Signaux clés:
- Impératifs directs à l'agent: "fais en sorte que...", "lance un server", "crée une branche", "checkes sur..."
- Spécifications d'implémentation: "tu peux mettre les statistiques en dessous", "ajoute un bouton call to action", "tu peux ajouter une section pour les crédits"
- Configuration technique: "variables d'environnement", noms de fichiers/branches spécifiques
- Commandes de debug: "casse ce qu'il faut casser", "envoie-moi le lien"

IMPORTANT: Quand tu vois une SÉRIE de 3+ segments consécutifs avec des impératifs ("fais...", "mets...", "ajoute...", "lance..."), SUPPRIME la série ENTIÈRE — c'est de la dictée.

⚠️ GARDE les interactions COURTES (1-2 segments) qui montrent le workflow au spectateur: vérification d'accès, lancement d'une tâche, réaction à un résultat. Garde aussi les descriptions HIGH-LEVEL de l'interface quand le locuteur décrit la STRUCTURE GÉNÉRALE d'un écran ("dans cet écran on va avoir le header, l'image, le titre").

### 4. Attente, débogage et processus en temps réel
SUPPRIME:
- Attente/vérification en direct: "je vais cliquer pour voir si...", "toujours pas", "il y a rien pour le moment", "on va attendre"
- Processus de debug: "casse ça", "corrige ça", "tu m'envoies le lien", "regarde dans les fichiers"
- Commentaires de processus: "je vais retourner ici", "je vais lui faire mon prompt", "il me demande si je suis sûr"
- Digressions et hors-sujet pendant les temps morts
⚠️ GARDE les moments où le locuteur MONTRE/NARRE un résultat au spectateur ("voilà, il a commencé à travailler, si on va sur l'application on peut voir..."). La différence: "attendre" = passif, "montrer" = narration active pour le spectateur.

### 5. Conclusions multiples
SUPPRIME toutes les tentatives de conclusion SAUF la TOUTE DERNIÈRE.

## CE QUE TU DOIS GARDER
- Contenu explicatif UNIQUE adressé au spectateur
- Démonstrations visuelles : le locuteur MONTRE ce qui est à l'écran
- Résultats et réactions : "ça marche", "voilà", "c'est tout bon" (quand c'est la première fois qu'on le dit)
- Évaluation de bugs quand ça fait partie de la démo : "la safe area ne marche pas", "les dimensions sont pas bonnes"
- Introduction et conclusion FINALE uniquement
- Descriptions HIGH-LEVEL de l'architecture/interface (sans entrer dans les détails d'implémentation)

## SEGMENTS ⟵ SUITE
Si un segment est supprimé, ses segments ⟵ SUITE doivent aussi être supprimés.

## {mode_instruction}"""

    algo_removed = len(chunks) - len(surviving)
    user_message = (
        f"Transcription pré-nettoyée ({len(surviving)} segments, "
        f"{algo_removed} déjà supprimés par l'algorithme). "
        f"Retourne les IDs à GARDER.\n\n{transcript}"
    )
    
    return system_prompt, user_message


def call_claude_api(system_prompt, user_message, api_key, use_thinking=True):
    """Call Claude API with the same structure as the Rust code"""
    import httpx
    
    tool = {
        "name": "report_keep_segments",
        "description": "Report which segments to keep in the final video",
        "input_schema": {
            "type": "object",
            "required": ["keep_ids"],
            "properties": {
                "keep_ids": {
                    "type": "array",
                    "items": {"type": "integer"},
                    "description": "List of segment IDs to keep in the final video, in chronological order"
                }
            }
        }
    }
    
    if use_thinking:
        request_body = {
            "model": "claude-sonnet-4-5-20250929",
            "max_tokens": 16000,
            "thinking": {
                "type": "enabled",
                "budget_tokens": 10000
            },
            "stream": True,
            "system": system_prompt,
            "tools": [tool],
            "tool_choice": {"type": "auto"},
            "messages": [{"role": "user", "content": user_message}]
        }
    else:
        request_body = {
            "model": "claude-sonnet-4-5-20250929",
            "max_tokens": 8192,
            "system": system_prompt,
            "tools": [tool],
            "tool_choice": {"type": "tool", "name": "report_keep_segments"},
            "messages": [{"role": "user", "content": user_message}]
        }
    
    request_size = len(json.dumps(request_body))
    print(f"  API request size: {request_size} chars ({request_size//1024}KB)")
    
    headers = {
        "x-api-key": api_key,
        "anthropic-version": "2023-06-01",
        "content-type": "application/json"
    }
    
    if use_thinking:
        print("  Streaming response (thinking enabled)...")
        thinking_text = ""
        tool_json = ""
        found_tool = False
        current_block_type = ""
        events_count = 0
        
        with httpx.Client(timeout=600) as client:
            with client.stream("POST", "https://api.anthropic.com/v1/messages",
                              headers=headers, json=request_body) as response:
                if response.status_code != 200:
                    error = response.read().decode()
                    raise Exception(f"API error ({response.status_code}): {error}")
                
                for line in response.iter_lines():
                    line = line.strip()
                    if not line.startswith("data: "):
                        continue
                    data = line[6:]
                    if data == "[DONE]":
                        break
                    
                    try:
                        event = json.loads(data)
                    except:
                        continue
                    
                    event_type = event.get("type", "")
                    events_count += 1
                    
                    if event_type == "content_block_start":
                        block = event.get("content_block", {})
                        current_block_type = block.get("type", "")
                        if current_block_type == "tool_use" and block.get("name") == "report_keep_segments":
                            found_tool = True
                    
                    elif event_type == "content_block_delta":
                        delta = event.get("delta", {})
                        delta_type = delta.get("type", "")
                        if delta_type == "thinking_delta":
                            thinking_text += delta.get("thinking", "")
                        elif delta_type == "input_json_delta" and found_tool:
                            tool_json += delta.get("partial_json", "")
                    
                    elif event_type == "content_block_stop":
                        if current_block_type == "thinking" and thinking_text:
                            print(f"  Thinking: {len(thinking_text)} chars")
                        current_block_type = ""
        
        print(f"  Processed {events_count} SSE events")
        
        if not tool_json:
            raise Exception(f"No tool_use found in streaming response ({events_count} events)")
        
        result = json.loads(tool_json)
        return result.get("keep_ids", []), thinking_text
    
    else:
        with httpx.Client(timeout=120) as client:
            response = client.post("https://api.anthropic.com/v1/messages",
                                   headers=headers, json=request_body)
            if response.status_code != 200:
                raise Exception(f"API error ({response.status_code}): {response.text}")
            
            result = response.json()
            for block in result.get("content", []):
                if block.get("type") == "tool_use" and block.get("name") == "report_keep_segments":
                    return block.get("input", {}).get("keep_ids", []), ""
            
            raise Exception("No tool_use block found in response")


# ──────────────────────────────────────────────────────────
# Evaluation
# ──────────────────────────────────────────────────────────

def evaluate_full(chunks, keep_ids, labels, exp_text):
    keep_set = set(keep_ids)
    gk = set(i for i, l in enumerate(labels) if l)
    gr = set(i for i, l in enumerate(labels) if not l)
    ar = set(c['id'] for c in chunks) - keep_set
    
    correct_keep = keep_set & gk
    correct_remove = ar & gr
    fp = ar & gk
    fn = keep_set & gr
    
    acc = (len(correct_keep) + len(correct_remove)) / len(chunks)
    
    kept_dur = sum(c['end'] - c['start'] for c in chunks if c['id'] in keep_set)
    expected_dur = sum(c['end'] - c['start'] for c in chunks if c['id'] in gk)
    
    kept_w = [norm(w['text']) for c in chunks if c['id'] in keep_set for w in c['words']]
    exp_w = [norm(w) for w in exp_text.split()]
    kept_w = [w for w in kept_w if w]
    exp_w = [w for w in exp_w if w]
    sm = difflib.SequenceMatcher(None, kept_w, exp_w, autojunk=False)
    m = sum(b.size for b in sm.get_matching_blocks())
    p = m / len(kept_w) if kept_w else 0
    r = m / len(exp_w) if exp_w else 0
    f1 = 2 * p * r / (p + r) if (p + r) > 0 else 0
    
    return {
        'accuracy': acc, 'fp': sorted(fp), 'fn': sorted(fn),
        'kept_dur': kept_dur, 'expected_dur': expected_dur,
        'precision': p, 'recall': r, 'f1': f1,
        'keep_count': len(keep_set), 'remove_count': len(ar),
        'gt_keep': len(gk), 'gt_remove': len(gr),
        'correct_keep': len(correct_keep), 'correct_remove': len(correct_remove),
    }


def main():
    mode = "moderate"
    use_thinking = True
    
    for arg in sys.argv[1:]:
        if arg in ("aggressive", "moderate", "conservative"):
            mode = arg
        elif arg == "--no-thinking":
            use_thinking = False
    
    # Load API key
    api_key = os.environ.get("ANTHROPIC_API_KEY", "")
    if not api_key:
        env_path = "/root/.openclaw/workspace/autotrim-desktop/.env"
        if os.path.exists(env_path):
            with open(env_path) as f:
                for line in f:
                    if line.startswith("ANTHROPIC_API_KEY="):
                        api_key = line.split("=", 1)[1].strip().strip('"').strip("'")
    
    if not api_key:
        print("ERROR: No ANTHROPIC_API_KEY found")
        sys.exit(1)
    
    print(f"=== Full Pipeline Test (mode={mode}, thinking={use_thinking}) ===\n")
    
    # Load data
    rw, ew, rt, et = load_data()
    chunks = chunk_words(rw, 500)
    labels = ground_truth(chunks, rw, ew)
    
    print(f"Chunks: {len(chunks)}, GT: {sum(labels)} keep / {len(labels)-sum(labels)} remove\n")
    
    # Phase 1: Algorithmic detection
    print("=== PHASE 1: Algorithmic Detection ===")
    phase1_keep = detect(chunks, verbose=False)
    phase1_m = evaluate_full(chunks, phase1_keep, labels, et)
    
    print(f"Phase 1: keep {phase1_m['keep_count']}, remove {phase1_m['remove_count']}")
    print(f"  Output: {phase1_m['kept_dur']/60:.1f} min (expected: {phase1_m['expected_dur']/60:.1f} min)")
    print(f"  Accuracy: {phase1_m['accuracy']*100:.1f}% | FP: {len(phase1_m['fp'])} | FN: {len(phase1_m['fn'])}")
    print(f"  Text: P={phase1_m['precision']*100:.1f}% R={phase1_m['recall']*100:.1f}% F1={phase1_m['f1']*100:.1f}%")
    
    # Phase 2: Claude API
    print(f"\n=== PHASE 2: Claude API (mode={mode}) ===")
    system_prompt, user_message = build_claude_prompt(chunks, phase1_keep, mode)
    
    print(f"  Surviving chunks for Claude: {phase1_m['keep_count']}")
    
    start_time = time.time()
    claude_keep_ids, thinking = call_claude_api(system_prompt, user_message, api_key, use_thinking)
    elapsed = time.time() - start_time
    print(f"  Claude API call took {elapsed:.1f}s")
    
    # Validate
    valid_keep = set(claude_keep_ids) & phase1_keep
    claude_removed = phase1_keep - valid_keep
    
    print(f"  Claude kept: {len(valid_keep)}/{len(phase1_keep)} surviving chunks")
    print(f"  Claude additionally removed: {len(claude_removed)} chunks")
    
    # Final evaluation
    print(f"\n=== FINAL RESULTS ===")
    final_m = evaluate_full(chunks, valid_keep, labels, et)
    
    print(f"Final: keep {final_m['keep_count']}, remove {final_m['remove_count']} | GT: keep {final_m['gt_keep']}, remove {final_m['gt_remove']}")
    print(f"Accuracy: {final_m['accuracy']*100:.1f}% | FP: {len(final_m['fp'])} | FN: {len(final_m['fn'])}")
    print(f"Output: {final_m['kept_dur']/60:.1f} min (expected: {final_m['expected_dur']/60:.1f} min)")
    print(f"Text: P={final_m['precision']*100:.1f}% R={final_m['recall']*100:.1f}% F1={final_m['f1']*100:.1f}%")
    
    if final_m['fp']:
        print(f"\nFP (wrongly removed - {len(final_m['fp'])}):")
        for cid in final_m['fp']:
            c = chunks[cid]
            print(f"  [{cid}] {c['start']:.0f}s ({c['word_count']}w): {c['text'][:80]}")
    
    if final_m['fn']:
        print(f"\nFN (wrongly kept - {len(final_m['fn'])}):")
        for cid in final_m['fn']:
            c = chunks[cid]
            in_phase1 = "P1-removed" if cid not in phase1_keep else "P2-missed"
            print(f"  [{cid}] {c['start']:.0f}s ({c['word_count']}w) [{in_phase1}]: {c['text'][:80]}")
    
    if claude_removed:
        print(f"\n--- Claude removed ({len(claude_removed)}) ---")
        for cid in sorted(claude_removed):
            c = chunks[cid]
            was_gt_keep = cid in set(i for i, l in enumerate(labels) if l)
            status = "✅ correct" if not was_gt_keep else "❌ FP!"
            print(f"  [{cid}] {c['start']:.0f}s ({c['word_count']}w) {status}: {c['text'][:80]}")
    
    target = 31.0
    if final_m['kept_dur']/60 <= target:
        print(f"\n✅ {final_m['kept_dur']/60:.1f} min ≤ {target} min — TARGET MET!")
    else:
        print(f"\n❌ {final_m['kept_dur']/60:.1f} min > {target} min ({final_m['kept_dur']/60 - target:.1f} min over)")
    
    # Save thinking for analysis
    if thinking:
        with open('last_claude_thinking.txt', 'w') as f:
            f.write(thinking)
        print(f"\nClaude thinking saved to last_claude_thinking.txt ({len(thinking)} chars)")
    
    return final_m


if __name__ == '__main__':
    main()
