#!/usr/bin/env python3
"""
Simulate the Rust/Claude pipeline in Python to analyze what it produces.

This script:
1. Loads the raw transcription
2. Segments into chunks (like segment_into_chunks in Rust)
3. Builds retake hints (like build_retake_hints in Rust)
4. Calls Claude to determine which chunks to keep
5. Analyzes the results
"""

import os
import sys
import json
import subprocess
from pathlib import Path
from anthropic import Anthropic

BASE_DIR = Path('/root/.openclaw/workspace/autotrim-desktop')
TEST_DIR = BASE_DIR / 'test_data'

def load_transcription(path):
    """Load an AssemblyAI transcription JSON file."""
    with open(path) as f:
        return json.load(f)

def segment_into_chunks(words, silence_threshold=0.5):
    """
    Segment words into chunks of continuous speech.
    Mimics the Rust function segment_into_chunks.
    """
    if not words:
        return []
    
    chunks = []
    current_chunk = []
    current_text = []
    chunk_start = words[0]['start'] / 1000.0  # Convert ms to seconds
    
    for i, word in enumerate(words):
        current_chunk.append(word)
        current_text.append(word['text'])
        
        # Check gap to next word
        if i + 1 < len(words):
            gap = (words[i + 1]['start'] - word['end']) / 1000.0
            
            if gap >= silence_threshold:
                # End this chunk
                chunk_end = word['end'] / 1000.0
                chunks.append({
                    'id': len(chunks),
                    'text': ' '.join(current_text),
                    'start': chunk_start,
                    'end': chunk_end,
                    'word_count': len(current_chunk),
                })
                current_chunk = []
                current_text = []
                if i + 1 < len(words):
                    chunk_start = words[i + 1]['start'] / 1000.0
        else:
            # Last word
            chunk_end = word['end'] / 1000.0
            chunks.append({
                'id': len(chunks),
                'text': ' '.join(current_text),
                'start': chunk_start,
                'end': chunk_end,
                'word_count': len(current_chunk),
            })
    
    return chunks

def build_retake_hints(chunks):
    """
    Build retake hints similar to the Rust implementation.
    This is a simplified version - the Rust version has more sophisticated detection.
    """
    hints = []
    
    # Detect obvious false starts: consecutive chunks with same opening words
    for i in range(len(chunks) - 1):
        curr_words = chunks[i]['text'].split()
        if len(curr_words) < 3:
            continue
        
        curr_opener = curr_words[:3]
        
        # Look ahead for similar openers
        for j in range(i + 1, min(i + 10, len(chunks))):
            if chunks[j]['start'] - chunks[i]['end'] > 120.0:
                break
            
            next_words = chunks[j]['text'].split()
            if len(next_words) < 3:
                continue
            
            next_opener = next_words[:3]
            
            if curr_opener == next_opener:
                hints.append(f"⚠️ REPRISE DÉTECTÉE : chunks [{i}] et [{j}] commencent tous deux par '{' '.join(curr_opener)}'. Probablement une reprise — garder SEULEMENT le dernier ({j}).")
                break
    
    if hints:
        return "\n## REPRISES PRÉ-DÉTECTÉES\n\n" + "\n".join(hints) + "\n\n"
    return ""

def call_claude(chunks, api_key):
    """
    Call Claude to determine which chunks to keep.
    Uses the same prompt as the Rust implementation.
    """
    client = Anthropic(api_key=api_key)
    
    retake_hints = build_retake_hints(chunks)
    
    # Build transcript
    transcript = []
    for i, chunk in enumerate(chunks):
        if i > 0:
            gap = chunk['start'] - chunks[i - 1]['end']
            if gap >= 1.0:
                transcript.append(f"  --- {gap:.1f}s ---")
        
        # Check if continuation (starts with lowercase)
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
    
    system_prompt = """Tu es un assistant de montage vidéo expert. Tu analyses la transcription brute d'un rush vidéo pour déterminer les moments à GARDER dans le montage final.

La transcription est découpée en segments de parole numérotés. Chaque segment est un bloc continu de parole. Les silences entre segments sont automatiquement supprimés.

## TON TRAVAIL
Utilise ton raisonnement interne (thinking) pour analyser SYSTÉMATIQUEMENT la transcription, puis retourne la liste des IDs de segments à GARDER via l'outil report_keep_segments.

### MÉTHODE D'ANALYSE (dans ton thinking) :
1. Parcours la transcription de haut en bas
2. Pour chaque zone, identifie si le locuteur fait des REPRISES (même sujet répété)
3. Pour chaque groupe de reprises, identifie la DERNIÈRE VERSION COMPLÈTE
4. Vérifie que tu ne gardes qu'UNE SEULE VERSION par passage
5. VÉRIFICATION FINALE : relis ta liste et pour tout segment gardé, demande-toi "est-ce que ce contenu est déjà dit ailleurs dans un segment que je garde aussi ?" Si oui, supprime le doublon.

## RÈGLE N°1 — REPRISES (la plus importante !)
Le locuteur fait souvent PLUSIEURS TENTATIVES pour dire la même chose. Il peut y avoir 2, 5, 10, voire 20 tentatives d'un même passage !

COMMENT DÉTECTER UNE REPRISE :
- Plusieurs segments commencent par les mêmes mots ou abordent le même sujet
- Le locuteur s'arrête, puis recommence avec une formulation similaire ou différente
- Les tentatives sont proches dans le temps (quelques secondes à quelques minutes d'écart)
- ⚠️ IMPORTANT : les reprises ne sont PAS toujours mot-pour-mot identiques ! Le locuteur peut REFORMULER complètement entre deux tentatives.

⚠️ MAIS ATTENTION : phrases similaires ≠ reprises ! Le locuteur utilise souvent les MÊMES EXPRESSIONS DE TRANSITION à différents moments :
- "Voilà ce dont je parlais..." peut apparaître à 5 endroits différents → PAS une reprise si sujets différents
- Une reprise = MÊME SUJET + MÊME CONTEXTE + proches dans le temps (<2 min d'écart)

## RÈGLE PRINCIPALE
Pour chaque groupe de reprises : garde UNIQUEMENT la DERNIÈRE tentative complète, supprime TOUTES les autres."""

    user_message = f"Voici la transcription du rush vidéo. Retourne les IDs des segments à GARDER.\n\n{retake_hints}{transcript_text}"
    
    print(f"Calling Claude with {len(chunks)} chunks...", file=sys.stderr)
    print(f"Transcript length: {len(transcript_text)} chars", file=sys.stderr)
    
    # Use extended thinking for better analysis
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
    
    # Extract keep_ids from tool use
    keep_ids = []
    thinking_text = None
    
    for block in response.content:
        if block.type == "thinking":
            thinking_text = block.thinking
        elif block.type == "tool_use" and block.name == "report_keep_segments":
            keep_ids = block.input.get("keep_ids", [])
    
    return keep_ids, thinking_text

def analyze_results(chunks, keep_ids, expected_duration=1806.0):
    """Analyze what Claude kept vs removed."""
    keep_set = set(keep_ids)
    
    total_kept_duration = sum(
        chunks[i]['end'] - chunks[i]['start']
        for i in keep_ids if i < len(chunks)
    )
    
    total_removed_duration = sum(
        chunk['end'] - chunk['start']
        for chunk in chunks if chunk['id'] not in keep_set
    )
    
    print(f"\n{'='*60}")
    print(f"RESULTS")
    print(f"{'='*60}")
    print(f"Total chunks: {len(chunks)}")
    print(f"Kept: {len(keep_ids)} ({len(keep_ids)/len(chunks)*100:.1f}%)")
    print(f"Removed: {len(chunks) - len(keep_ids)} ({(len(chunks)-len(keep_ids))/len(chunks)*100:.1f}%)")
    print(f"\nKept speech duration: {total_kept_duration:.1f}s ({total_kept_duration/60:.1f}min)")
    print(f"Removed speech duration: {total_removed_duration:.1f}s ({total_removed_duration/60:.1f}min)")
    print(f"Expected duration: {expected_duration:.1f}s ({expected_duration/60:.1f}min)")
    print(f"\nDuration difference: {total_kept_duration - expected_duration:.1f}s")
    print(f"Duration ratio: {total_kept_duration / expected_duration * 100:.1f}%")
    
    if total_kept_duration / expected_duration > 1.05:
        print(f"\n❌ TOO LONG: Output would be {(total_kept_duration / expected_duration - 1) * 100:.1f}% longer than expected")
    elif total_kept_duration / expected_duration < 0.95:
        print(f"\n❌ TOO SHORT: Output would be {(1 - total_kept_duration / expected_duration) * 100:.1f}% shorter than expected")
    else:
        print(f"\n✅ GOOD: Duration within ±5% of expected")
    
    return {
        'kept_count': len(keep_ids),
        'removed_count': len(chunks) - len(keep_ids),
        'kept_duration': total_kept_duration,
        'removed_duration': total_removed_duration,
        'expected_duration': expected_duration,
        'duration_ratio': total_kept_duration / expected_duration,
    }

def main():
    api_key = os.environ.get('ANTHROPIC_API_KEY')
    if not api_key:
        print("ERROR: ANTHROPIC_API_KEY not set", file=sys.stderr)
        sys.exit(1)
    
    print("Loading raw transcription...")
    raw_trans = load_transcription(TEST_DIR / 'raw_transcription.json')
    exp_trans = load_transcription(TEST_DIR / 'expected_transcription.json')
    
    raw_words = raw_trans.get('words', [])
    exp_words = exp_trans.get('words', [])
    
    exp_duration = exp_words[-1]['end'] / 1000.0 if exp_words else 1806.0
    
    print(f"Raw: {len(raw_words)} words")
    print(f"Expected: {len(exp_words)} words, {exp_duration:.1f}s")
    
    # Segment into chunks
    print("\nSegmenting into chunks...")
    chunks = segment_into_chunks(raw_words, silence_threshold=0.5)
    print(f"Created {len(chunks)} chunks")
    
    # Save chunks for inspection
    chunks_path = TEST_DIR / 'reports' / 'rust_sim_chunks.json'
    with open(chunks_path, 'w') as f:
        json.dump(chunks, f, indent=2, ensure_ascii=False)
    print(f"Chunks saved to {chunks_path}")
    
    # Call Claude
    print("\nCalling Claude to analyze retakes...")
    keep_ids, thinking = call_claude(chunks, api_key)
    
    # Save thinking
    if thinking:
        thinking_path = TEST_DIR / 'reports' / 'rust_sim_thinking.txt'
        with open(thinking_path, 'w') as f:
            f.write(thinking)
        print(f"Claude thinking saved to {thinking_path}")
    
    # Save keep_ids
    keep_ids_path = TEST_DIR / 'reports' / 'rust_sim_keep_ids.json'
    with open(keep_ids_path, 'w') as f:
        json.dump(keep_ids, f, indent=2)
    print(f"Keep IDs saved to {keep_ids_path}")
    
    # Analyze
    results = analyze_results(chunks, keep_ids, exp_duration)
    
    # Save full report
    report_path = TEST_DIR / 'reports' / 'rust_sim_report.json'
    with open(report_path, 'w') as f:
        json.dump({
            'chunks_count': len(chunks),
            'keep_ids': keep_ids,
            'results': results,
        }, f, indent=2)
    print(f"\nFull report saved to {report_path}")

if __name__ == '__main__':
    main()
