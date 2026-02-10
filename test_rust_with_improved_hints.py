#!/usr/bin/env python3
"""
Test Claude with improved retake hints.
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

def call_claude_with_improved_hints(chunks, api_key):
    """Call Claude with improved hints."""
    client = Anthropic(api_key=api_key)
    
    # Build advanced hints
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
    
    # Improved system prompt - more aggressive about retakes
    system_prompt = """Tu es un assistant de montage vidéo expert. Tu analyses la transcription brute d'un rush vidéo pour déterminer les moments à GARDER dans le montage final.

La transcription est découpée en segments de parole numérotés. Chaque segment est un bloc continu de parole. Les silences entre segments sont automatiquement supprimés.

## TON TRAVAIL
Utilise ton raisonnement interne (thinking) pour analyser la transcription, puis retourne la liste des IDs de segments à GARDER via l'outil report_keep_segments.

## RÈGLE ABSOLUE — REPRISES PRÉ-DÉTECTÉES
Les groupes de reprises listés ci-dessous ont été détectés algorithmiquement avec un très haut niveau de confiance.
**TU DOIS SUIVRE CES INDICATIONS À LA LETTRE** : pour chaque groupe, garde UNIQUEMENT le dernier chunk indiqué, supprime TOUS les autres.

❌ NE PAS garder plusieurs chunks d'un même groupe de reprises.
✅ Garder SEULEMENT le dernier chunk de chaque groupe.

## RÈGLE N°1 — Reprises non détectées algorithmiquement
En PLUS des reprises pré-détectées, cherche d'autres reprises que l'algorithme aurait manquées :
- Plusieurs segments abordent le même sujet (même si les mots sont différents)
- Le locuteur reformule complètement entre tentatives
- Tentatives proches dans le temps (<2 min d'écart)

Pour chaque reprise trouvée : garde UNIQUEMENT la DERNIÈRE tentative complète.

## RÈGLE N°2 — Segments qui se continuent (⟵ SUITE)
Les segments marqués "⟵ SUITE" sont la CONTINUATION du segment précédent.
→ Tu ne peux JAMAIS supprimer [N] et garder [N+1] si [N+1] est marqué SUITE.
→ Garder ou supprimer le BLOC ENTIER (les deux ensemble).

## RÈGLE N°3 — Faux départs
Segments très courts (<5 mots) suivis d'un segment similaire plus long → presque toujours un faux départ → supprimer.

## RÈGLE N°4 — Contenu unique
Tout contenu UNIQUE (dit une seule fois, pas dans un groupe de reprises) → GARDER.

Mode de travail : AGGRESSIF — supprime toutes les reprises détectées."""

    user_message = f"Voici la transcription du rush vidéo. Retourne les IDs des segments à GARDER.\n\n{retake_hints}{transcript_text}"
    
    print(f"Calling Claude with improved hints ({len(chunks)} chunks)...", file=sys.stderr)
    print(f"Hints length: {len(retake_hints)} chars", file=sys.stderr)
    
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
    
    # Load chunks
    chunks = load_json(REPORTS_DIR / 'rust_sim_chunks.json')
    exp_trans = load_json(TEST_DIR / 'expected_transcription.json')
    exp_words = exp_trans.get('words', [])
    exp_duration = exp_words[-1]['end'] / 1000.0
    
    print(f"Loaded {len(chunks)} chunks")
    print(f"Expected duration: {exp_duration:.1f}s ({exp_duration/60:.1f}min)")
    
    # Call Claude with improved hints
    keep_ids, thinking = call_claude_with_improved_hints(chunks, api_key)
    
    # Save results
    if thinking:
        with open(REPORTS_DIR / 'rust_improved_thinking.txt', 'w') as f:
            f.write(thinking)
        print(f"Thinking saved to rust_improved_thinking.txt")
    
    with open(REPORTS_DIR / 'rust_improved_keep_ids.json', 'w') as f:
        json.dump(keep_ids, f, indent=2)
    print(f"Keep IDs saved to rust_improved_keep_ids.json")
    
    # Analyze
    total_kept_duration = sum(
        chunks[i]['end'] - chunks[i]['start']
        for i in keep_ids if i < len(chunks)
    )
    
    print(f"\n{'='*60}")
    print(f"RESULTS (WITH IMPROVED HINTS)")
    print(f"{'='*60}")
    print(f"Kept: {len(keep_ids)}/{len(chunks)} ({len(keep_ids)/len(chunks)*100:.1f}%)")
    print(f"Kept duration: {total_kept_duration:.1f}s ({total_kept_duration/60:.1f}min)")
    print(f"Expected duration: {exp_duration:.1f}s ({exp_duration/60:.1f}min)")
    print(f"Difference: {total_kept_duration - exp_duration:.1f}s ({(total_kept_duration - exp_duration)/60:.1f}min)")
    print(f"Ratio: {total_kept_duration / exp_duration * 100:.1f}%")
    
    if total_kept_duration / exp_duration > 1.05:
        print(f"\n❌ TOO LONG: {(total_kept_duration / exp_duration - 1) * 100:.1f}% longer")
    elif total_kept_duration / exp_duration < 0.95:
        print(f"\n❌ TOO SHORT: {(1 - total_kept_duration / exp_duration) * 100:.1f}% shorter")
    else:
        print(f"\n✅ GOOD: Within ±5% of expected!")

if __name__ == '__main__':
    main()
