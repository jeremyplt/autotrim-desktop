#!/usr/bin/env python3
"""
Test Claude with ULTRA AGGRESSIVE mode.
"""

import os
import sys
import json
from pathlib import Path
from anthropic import Anthropic
sys.path.insert(0, str(Path(__file__).parent))
from improved_retake_detection import build_advanced_hints

TEST_DIR = Path('/root/.openclaw/workspace/autotrim-desktop/test_data')
REPORTS_DIR = TEST_DIR / 'reports'

def load_json(path):
    with open(path) as f:
        return json.load(f)

def call_claude_ultra_aggressive(chunks, api_key):
    """Call Claude with ULTRA AGGRESSIVE prompt."""
    client = Anthropic(api_key=api_key)
    
    retake_hints = build_advanced_hints(chunks)
    
    # Build transcript
    transcript = []
    for i, chunk in enumerate(chunks):
        if i > 0:
            gap = chunk['start'] - chunks[i - 1]['end']
            if gap >= 1.0:
                transcript.append(f"  --- {gap:.1f}s ---")
        
        continuation_marker = " ⟵ SUITE" if chunk['text'][0].islower() else ""
        
        mins = int(chunk['start'] / 60)
        secs = chunk['start'] % 60
        end_mins = int(chunk['end'] / 60)
        end_secs = chunk['end'] % 60
        
        transcript.append(
            f"[{chunk['id']}] {mins}:{secs:05.2f}-{end_mins}:{end_secs:05.2f} "
            f"({chunk['end'] - chunk['start']:.1f}s, {chunk['word_count']} mots){continuation_marker} "
            f"{chunk['text']}"
        )
    
    transcript_text = "\n".join(transcript)
    
    # ULTRA AGGRESSIVE SYSTEM PROMPT
    system_prompt = """Tu es un assistant de montage vidéo ULTRA-AGRESSIF. Ton objectif est de SUPPRIMER IMPITOYABLEMENT toutes les reprises, hésitations, et tentatives ratées.

**MODE : ULTRA-AGRESSIF**
Préfère supprimer trop que pas assez. En cas de doute : SUPPRIME.

## RÈGLE ABSOLUE N°1 — REPRISES PRÉ-DÉTECTÉES

Les groupes ci-dessous sont des REPRISES CONFIRMÉES.

**OBLIGATION STRICTE** : Pour chaque groupe, garde UNIQUEMENT le dernier chunk listé.
**INTERDIT** : Garder 2+ chunks d'un même groupe.
**PAS D'EXCEPTION** : Même si tu penses qu'un chunk intermédiaire est bon, SUPPRIME-LE.

## RÈGLE ABSOLUE N°2 — CHASSE AUX REPRISES NON-DÉTECTÉES

Au-delà des reprises pré-détectées, CHERCHE ACTIVEMENT d'autres reprises :

**Critère de reprise** (SI L'UN DE CES CRITÈRES EST REMPLI = REPRISE) :
1. **Deux chunks parlent du même sujet** dans une fenêtre de 2 minutes
   → Garde SEULEMENT le plus complet/le dernier
2. **Deux chunks commencent par des mots similaires** (même racine)
   → Exemple : "Et puis surtout" / "et puis surtout" = MÊME chose
   → Garde SEULEMENT le dernier
3. **Chunk très court (<8 mots) suivi d'un chunk similaire**
   → Le court est un faux départ → SUPPRIME
4. **Chunk se termine par "—" ou semble incomplet**
   → Tentative abandonnée → SUPPRIME

## RÈGLE ABSOLUE N°3 — SEGMENTS ⟵ SUITE

Chunks marqués "⟵ SUITE" = continuation du précédent.
**BLOC INDIVISIBLE** : Garde ou supprime [N] ET [N+1] ensemble.

## RÈGLE ABSOLUE N°4 — PROCESSUS EN 2 PASSES

**PASSE 1** : Marque tous les chunks à SUPPRIMER selon les règles ci-dessus.

**PASSE 2 (VÉRIFICATION ANTI-DOUBLON)** :
Pour CHAQUE chunk que tu as marqué "garder" :
1. Est-ce que ce contenu est déjà présent dans un AUTRE chunk que tu gardes ?
2. Si OUI → Garde SEULEMENT le plus complet/le dernier, SUPPRIME l'autre

**PASSE 3 (VÉRIFICATION FINALE)** :
Relis ta liste finale. Si tu vois 2 chunks qui parlent du même sujet → c'est une ERREUR, supprime le premier.

## TON OBJECTIF DE DURÉE

La vidéo brute fait ~100 minutes. Le montage final doit faire ~30 minutes.
→ Tu dois SUPPRIMER ~70% du contenu.
→ Si tu gardes >40% des chunks, tu n'es PAS assez agressif.

## EN CAS DE DOUTE

Doute si chunk est une reprise ? → **SUPPRIME.**
Doute si chunk est utile ? → **SUPPRIME.**
Chunk semble OK mais ressemble à un autre ? → **GARDE LE DERNIER, SUPPRIME LE PREMIER.**

Ton mantra : **"QUAND ON DOUTE, ON SUPPRIME."**"""

    user_message = f"Voici la transcription. Retourne les IDs à GARDER. Sois IMPITOYABLE.\n\n{retake_hints}{transcript_text}"
    
    print(f"Calling Claude ULTRA AGGRESSIVE...", file=sys.stderr)
    
    response = client.messages.create(
        model="claude-sonnet-4-20250514",
        max_tokens=16000,
        thinking={
            "type": "enabled",
            "budget_tokens": 10000
        },
        system=system_prompt,
        tools=[{
            "name": "report_keep_segments",
            "description": "Report which segments to keep",
            "input_schema": {
                "type": "object",
                "required": ["keep_ids"],
                "properties": {
                    "keep_ids": {
                        "type": "array",
                        "items": {"type": "integer"}
                    }
                }
            }
        }],
        messages=[{"role": "user", "content": user_message}]
    )
    
    keep_ids = []
    thinking_text = None
    
    for block in response.content:
        if block.type == "thinking":
            thinking_text = block.thinking
        elif block.type == "tool_use" and block.name == "report_keep_segments":
            keep_ids = block.input.get("keep_ids", [])
    
    return keep_ids, thinking_text

def main():
    api_key = os.environ.get('ANTHROPIC_API_KEY')
    if not api_key:
        print("ERROR: ANTHROPIC_API_KEY not set", file=sys.stderr)
        sys.exit(1)
    
    chunks = load_json(REPORTS_DIR / 'rust_sim_chunks.json')
    exp_trans = load_json(TEST_DIR / 'expected_transcription.json')
    exp_words = exp_trans.get('words', [])
    exp_duration = exp_words[-1]['end'] / 1000.0
    
    print(f"Loaded {len(chunks)} chunks")
    
    keep_ids, thinking = call_claude_ultra_aggressive(chunks, api_key)
    
    if thinking:
        with open(REPORTS_DIR / 'rust_ultra_thinking.txt', 'w') as f:
            f.write(thinking)
    
    with open(REPORTS_DIR / 'rust_ultra_keep_ids.json', 'w') as f:
        json.dump(keep_ids, f, indent=2)
    
    total_kept_duration = sum(
        chunks[i]['end'] - chunks[i]['start']
        for i in keep_ids if i < len(chunks)
    )
    
    print(f"\n{'='*60}")
    print(f"ULTRA AGGRESSIVE RESULTS")
    print(f"{'='*60}")
    print(f"Kept: {len(keep_ids)}/{len(chunks)} ({len(keep_ids)/len(chunks)*100:.1f}%)")
    print(f"Removed: {len(chunks) - len(keep_ids)}/{len(chunks)} ({(len(chunks)-len(keep_ids))/len(chunks)*100:.1f}%)")
    print(f"Kept duration: {total_kept_duration:.1f}s ({total_kept_duration/60:.1f}min)")
    print(f"Expected duration: {exp_duration:.1f}s ({exp_duration/60:.1f}min)")
    print(f"Difference: {total_kept_duration - exp_duration:.1f}s ({(total_kept_duration - exp_duration)/60:.1f}min)")
    print(f"Ratio: {total_kept_duration / exp_duration * 100:.1f}%")
    
    if abs(total_kept_duration - exp_duration) / exp_duration < 0.01:
        print(f"\n✅ EXCELLENT: Within ±1% of expected!")
    elif abs(total_kept_duration - exp_duration) / exp_duration < 0.05:
        print(f"\n✅ GOOD: Within ±5% of expected")
    else:
        ratio_diff = (total_kept_duration / exp_duration - 1) * 100
        if ratio_diff > 0:
            print(f"\n❌ Still too long: {ratio_diff:.1f}% longer")
        else:
            print(f"\n❌ Too short: {-ratio_diff:.1f}% shorter")

if __name__ == '__main__':
    main()
