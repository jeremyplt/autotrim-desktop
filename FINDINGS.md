# AutoTrim - Diagnostic et Solution

## 📊 Résumé Exécutif

**Problème identifié** : Le pipeline Rust/Claude produit 39.7 min au lieu de 30.1 min (+32%)
**Cause racine** : Claude garde 56 chunks de reprises qu'il devrait supprimer (+10.4 min)
**Solution développée** : Algorithme de détection avancé + prompt amélioré
**Résultat** : 39.7 min → 34.0 min (amélioration de 5.7 min, écart réduit à +13%)

---

## 🔍 Analyse Détaillée du Problème

### Comparaison des Pipelines

| Pipeline | Durée output | vs Expected | Chunks gardés | Efficacité |
|----------|--------------|-------------|---------------|------------|
| Expected (manuel) | 30.1 min | baseline | - | 100% |
| Python (difflib) | 30.5 min | +1.5% | 86 segments | ✅ 98.5% |
| Rust/Claude (actuel) | 39.7 min | +31.7% | 180/246 chunks | ❌ 67% |
| **Rust/Claude (amélioré)** | **34.0 min** | **+12.9%** | **151/246 chunks** | **✅ 87%** |

### Pourquoi le Pipeline Claude Échoue

#### 1. Détection Algorithmique Insuffisante

**Code actuel** (`build_retake_hints` dans `analysis.rs`) :
- Détecte uniquement les reprises avec **exactement les 3 mêmes mots** au début
- Manque les reprises reformulées

**Exemples de reprises manquées** :
```
[14] "Et puis surtout, Ralfloop, c'est rien de bien sorcier..."
[16] "Et puis surtout ralfloop en fait il n'y a pas vraiment de valeur ajoutée..."
[20] "et puis surtout ralfloop en fait il n'y a vraiment aucune valeur ajoutée..."
```
→ Chunks 14, 16, 20 disent la MÊME chose mais avec des variantes de formulation
→ L'algorithme actuel ne les groupe PAS ensemble
→ Claude garde les 3, alors qu'il devrait garder SEULEMENT 20

#### 2. Prompt Claude Trop Conservateur

Le prompt actuel contient :
- ❌ "PIÈGE À ÉVITER : phrases similaires ≠ reprises !" — rend Claude trop prudent
- ❌ Trop de nuances et cas limites — Claude hésite et garde par défaut
- ❌ Manque de directives claires sur les hints pré-détectés

#### 3. Faux Positifs Spécifiques Identifiés

**56 chunks gardés à tort**, notamment :
- Chunks 14, 15, 16, 19 : reprises "ralfloop" (4 versions gardées au lieu de 1)
- Chunks 22, 23, 25 : reprises "pas super intéressant" (3 versions gardées au lieu de 1)
- Chunks 53, 54, 55, 56 : reprises "setup OpenClose" (4 versions gardées au lieu de 2)
- Chunks 40, 41, 43 : reprises "OpenClo disponible sur Hostinger"

**Durée totale des faux positifs** : 626 secondes (10.4 minutes)

---

## ✅ Solution Développée

### Approche Hybride : Algorithme Avancé + Prompt Optimisé

#### 1. Algorithme de Détection Amélioré

**Fichier** : `improved_retake_detection.py` (prototype Python)

**Méthode** :
```python
def ngram_similarity(text1, text2, n=3):
    """Compare tri-grammes de mots pour détecter similarité"""
    ngrams1 = set(get_ngrams(text1, n))
    ngrams2 = set(get_ngrams(text2, n))
    return len(ngrams1 & ngrams2) / len(ngrams1 | ngrams2)

def detect_retake_groups_advanced(chunks):
    """Détecte groupes de reprises via similarité de contenu"""
    for i, chunk_i in enumerate(chunks):
        for j in range(i+1, len(chunks)):
            if chunks[j].start - chunk_i.end > 180:  # 3 min window
                break
            similarity = ngram_similarity(chunk_i.text, chunks[j].text)
            if similarity > 0.35:  # threshold
                # Marquer comme groupe de reprises
```

**Performance** (avec seuil 0.35) :
- Détecte 43 groupes de reprises (vs 19 avec l'algorithme actuel)
- Précision : 54.7% | Recall : 73.9% | F1 : 62.9%

#### 2. Hints Améliorés pour Claude

**Format explicite** :
```
⚠️ GROUPE DE REPRISES #1:
   Chunks: [14, 15, 16, 19]
   → GARDER SEULEMENT: [20]
   → SUPPRIMER: [14, 15, 16, 19]
   
  [14] "Et puis surtout, Ralfloop, c'est rien de bien sorcier..."
  [15] "et puis surtout, Ralfloop, c'est rien de bien compliqué..."
  [16] "Et puis surtout ralfloop en fait il n'y a pas de valeur..."
  [19] "Et puis surtout ralfloop en fait c'est rien du tout..."
  [20] "et puis surtout ralfloop en fait il n'y a vraiment aucune valeur..." ← GARDER
```

**Avantages** :
- ✅ Non ambigu : dit exactement quoi faire
- ✅ Confiance élevée : "détecté algorithmiquement"
- ✅ Prévisualisation : Claude peut vérifier le contenu

#### 3. Prompt Claude Simplifié et Plus Directif

**Fichier** : `src-tauri/src/transcription/IMPROVED_PROMPT.txt`

**Changements clés** :
1. ✅ **RÈGLE N°1 (PRIORITÉ ABSOLUE)** — Suivre les hints sans exception
2. ✅ **Mode AGRESSIF** — Préférer supprimer en cas de doute
3. ❌ Suppression de "PIÈGE : phrases similaires ≠ reprises" (trop conservateur)
4. ✅ Clarification : "contenu unique" = dit une seule fois, pas de reformulation

**Résultat** : Prompt passé de ~2500 mots à ~800 mots, plus focalisé

---

## 📈 Résultats des Tests

### Test 1 : Pipeline Actuel (Baseline)
```
Durée : 39.7 min
Ratio : 131.7% de expected
Chunks gardés : 180/246 (73%)
Verdict : ❌ Trop long (+9.6 min)
```

### Test 2 : Hints Améliorés + Prompt Optimisé
```
Durée : 34.0 min
Ratio : 112.9% de expected
Chunks gardés : 151/246 (61%)
Verdict : ✅ Amélioration majeure (-5.7 min)
Écart restant : +3.9 min (13%)
```

### Test 3 : Ultra-Agressif (pour comparaison)
```
Durée : 15.8 min
Ratio : 52.5% de expected
Chunks gardés : 74/246 (30%)
Verdict : ❌ Trop agressif (-14.3 min)
```

**Conclusion** : La version "Hints Améliorés" est le meilleur équilibre

---

## 🎯 Recommandations

### Option A : Implémentation Rapide (Prompt Uniquement)

**Changement minimal** : Remplacer le prompt dans `analysis.rs` par la version simplifiée

**Fichiers à modifier** :
1. `src-tauri/src/transcription/analysis.rs` (lignes ~1090-1258)
   - Remplacer `system_prompt` par le contenu de `IMPROVED_PROMPT.txt`

**Gain attendu** : ~20-30% d'amélioration (écart passant de +32% à ~20-25%)

**Effort** : 15 minutes

### Option B : Implémentation Complète (Algorithme + Prompt)

**Changements** :
1. Porter l'algorithme de détection avancé de Python vers Rust
2. Intégrer le nouveau prompt
3. Ajuster les paramètres (seuil de similarité, fenêtre temporelle)

**Fichiers à modifier** :
1. `src-tauri/src/transcription/analysis.rs` :
   - Ajouter fonction `ngram_similarity()`
   - Ajouter fonction `detect_retake_groups_advanced()`
   - Modifier `build_retake_hints()` pour utiliser le nouvel algorithme
   - Remplacer le prompt

**Gain attendu** : ~60-70% d'amélioration (écart passant de +32% à ~13%)

**Effort** : 2-4 heures

### Option C : Approche Hybride (Python Script + Rust)

**Concept** :
1. Exécuter le script Python `improved_retake_detection.py` pour générer des hints
2. Passer ces hints au pipeline Rust/Claude
3. Garder le pipeline Rust existant pour le reste

**Avantage** : Implémentation rapide, pas de réécriture Rust
**Inconvénient** : Dépendance Python dans le pipeline

---

## 📦 Fichiers Créés

### Scripts de Test et d'Analyse
- `test_rust_pipeline.py` — Simule le pipeline Rust/Claude
- `improved_retake_detection.py` — Algorithme de détection avancé
- `test_rust_with_improved_hints.py` — Test avec hints améliorés
- `compare_pipelines.py` — Comparaison Python vs Rust
- `compare_improved_results.py` — Analyse des améliorations

### Documentation
- `ANALYSIS.md` — Analyse technique détaillée
- `FINDINGS.md` — Ce document
- `final_improved_prompt.txt` — Prompt optimisé (version standalone)
- `src-tauri/src/transcription/IMPROVED_PROMPT.txt` — Version pour intégration Rust

### Données de Test (test_data/reports/)
- `rust_sim_chunks.json` — Chunks générés par le segmenteur
- `rust_sim_keep_ids.json` — IDs gardés par Claude (baseline)
- `rust_improved_keep_ids.json` — IDs gardés avec hints améliorés
- `pipeline_comparison.json` — Comparaison détaillée baseline vs ground truth
- `improved_analysis.json` — Analyse de la version améliorée
- `advanced_retake_hints.txt` — Exemple de hints générés par l'algorithme avancé

---

## 🚀 Prochaines Étapes Recommandées

### Court Terme (Aujourd'hui)
1. ✅ Tester la version améliorée avec les données de test (FAIT)
2. ⏳ Décider quelle option implémenter (A, B, ou C)
3. ⏳ Implémenter les changements dans le code Rust
4. ⏳ Tester sur le fichier `raw.mov` complet
5. ⏳ Commit + push vers GitHub

### Moyen Terme (Cette Semaine)
1. Valider sur d'autres vidéos de Jeremy
2. Ajuster les paramètres si nécessaire (seuil de similarité, fenêtre temporelle)
3. Optimiser les performances (si l'algorithme avancé ralentit trop)

### Long Terme
1. Ajouter une UI pour que Jeremy puisse :
   - Choisir le mode (conservateur / modéré / agressif)
   - Visualiser les groupes de reprises détectés
   - Corriger manuellement si besoin
2. Entraîner un modèle ML sur les données de Jeremy pour améliorer la détection

---

## 🎓 Leçons Apprises

1. **Claude seul n'est pas suffisant** — Besoin d'un algorithme pré-traitement fort
2. **Les hints explicites fonctionnent mieux** que les instructions générales
3. **Équilibre agressif/conservateur crucial** — Trop agressif supprime du contenu utile
4. **La reformulation est le piège #1** — Même contenu, mots différents
5. **N-grams + difflib détectent bien les reformulations** — Approche complémentaire à l'exact-match

---

## 📊 Métriques de Succès

| Métrique | Avant | Après (Improved) | Cible | Status |
|----------|-------|------------------|-------|--------|
| Durée output | 39.7 min | 34.0 min | 30.1 min | 🟡 |
| Écart vs expected | +31.7% | +12.9% | ±5% | 🟡 |
| Chunks correctement gardés | 124/180 (69%) | 111/151 (73%) | >95% | 🟡 |
| False positives | 56 chunks | 40 chunks | <10 | 🟡 |
| False negatives | 10 chunks | 23 chunks | <10 | 🟡 |

**Légende** : 🟢 Atteint | 🟡 En progrès | 🔴 Non atteint

**Conclusion** : Amélioration significative (+60%), mais marge de progression restante pour atteindre ±5%

---

*Rapport généré le 2026-02-10 par le subagent fix-autotrim*
