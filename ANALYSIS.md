# AutoTrim Pipeline Analysis & Fix

## Problème identifié

Le pipeline Rust/Claude produit **39.7 minutes** au lieu de **30.1 minutes** (durée attendue).
- Excès de durée : **+9.6 minutes** (+31.7%)
- Cause : Claude garde **56 chunks** qui sont des **reprises ratées**
- Ces 56 chunks ajoutent **626 secondes** (10.4 min) de contenu inutile

## Pourquoi le pipeline Claude échoue

### 1. Détection algorithmique trop faible
Le code Rust actuel (`build_retake_hints`) ne détecte que les reprises avec :
- **Exactement les 3 mêmes mots** au début
- Exemple : détecte `[2]` et `[9]` qui commencent tous deux par "Alors pour régler"
- **Mais manque** : `[14]` "Et puis surtout, Ralfloop" vs `[16]` "Et puis surtout ralfloop en fait"

### 2. Claude est trop conservateur
Même avec les hints basiques, Claude :
- Hésite à supprimer du contenu
- Interprète trop littéralement la règle "phrases similaires ≠ reprises"
- Garde plusieurs versions d'un même passage par prudence

### 3. Exemples concrets de reprises manquées

**Exemple 1 : Ralfloop**
- `[14]` (239s-251s) : "Et puis surtout, Ralfloop, c'est rien de bien **sorcier**..."
- `[16]` (281s-312s) : "Et puis surtout ralfloop en fait il n'y a pas vraiment de **valeur ajoutée**..."
- `[20]` (331s-344s) : "et puis surtout ralfloop en fait il n'y a vraiment **aucune** valeur ajoutée..."

→ **Claude garde 14, 16 ET 20** alors qu'il devrait garder **SEULEMENT 20**

**Exemple 2 : "pas super intéressant"**
- `[22]` (385s-391s) : "Donc voilà, pas super intéressant..."
- `[23]` (391s-402s) : "donc voilà, pas très intéressant..."
- `[25]` (411s-424s) : "donc voilà, c'est pas super intéressant..."

→ **Claude garde les 3** alors qu'il devrait garder **SEULEMENT 25**

**Exemple 3 : Setup OpenClo**
- `[53]` (1171s-1185s) : "Et là, on arrive directement sur le setup de OpenClose..."
- `[54]` (1224s-1229s) : "Et là, on arrive directement sur le setup de OpenClose..."
- `[55]` (1230s-1234s) : "Donc là, maintenant, on a juste à suivre les instructions..."

→ **Claude garde les 3** alors qu'il devrait garder **SEULEMENT 55-56**

## Solution mise en place

### Approche hybride : Algorithme avancé + Claude

#### 1. Détection algorithmique améliorée

**Méthode** : Similarité de contenu (n-grams + difflib)
```python
def ngram_similarity(text1, text2, n=3):
    # Compare les tri-grammes de mots
    # Détecte les reprises même reformulées
```

**Résultats** :
- Seuil 0.35 : Détecte **43 groupes de reprises**
- Garde 181 chunks (17.5 min) — encore trop mais meilleur
- F1 score : 62.9% (vs 0% pour l'algo basique)

#### 2. Hints améliorés pour Claude

Format :
```
⚠️ GROUPE DE REPRISES #1:
   Chunks: [2, 3, 4, 9]
   → GARDER SEULEMENT: [9]
   → SUPPRIMER: [2, 3, 4]
```

Avantages :
- **Explicite** : dit exactement quoi garder/supprimer
- **Confiance** : "détecté algorithmiquement avec haut niveau de confiance"
- **Non ambigu** : pas d'interprétation possible

#### 3. Prompt Claude amélioré

Changements clés :
- ✅ "**TU DOIS SUIVRE CES INDICATIONS À LA LETTRE**" (plus directif)
- ✅ "Mode AGGRESSIF — supprime toutes les reprises détectées"
- ✅ Suppression de la section "phrases similaires ≠ reprises" (trop conservatrice)

## Plan de portage vers Rust

### Étape 1 : Améliorer `build_retake_hints` dans `analysis.rs`

Ajouter détection par similarité de contenu :
```rust
fn calculate_ngram_similarity(text1: &str, text2: &str, n: usize) -> f64 {
    // Implémenter n-gram similarity
}

fn detect_retake_groups_advanced(chunks: &[SpeechChunk]) -> Vec<RetakeGroup> {
    // Pour chaque paire de chunks dans une fenêtre de temps
    // Si similarité > seuil → groupe de reprises
}
```

### Étape 2 : Formater les hints de façon plus directive

```rust
fn build_retake_hints(chunks: &[SpeechChunk]) -> String {
    let groups = detect_retake_groups_advanced(chunks);
    
    let mut hints = String::from("## REPRISES PRÉ-DÉTECTÉES\n\n");
    for group in groups {
        hints.push_str(&format!(
            "⚠️ GROUPE : {}\n   → GARDER SEULEMENT: [{}]\n   → SUPPRIMER: {:?}\n\n",
            group.description, group.keep_last(), group.remove_all_but_last()
        ));
    }
    hints
}
```

### Étape 3 : Simplifier le prompt Claude

Supprimer les sections qui rendent Claude trop conservateur :
- ❌ Enlever "PIÈGE À ÉVITER : phrases similaires ≠ reprises"
- ❌ Enlever les nuances et cas limites
- ✅ Garder : "SUIS LES HINTS À LA LETTRE"
- ✅ Ajouter : "Mode AGGRESSIF"

## Tests de validation

### Test 1 : Python avec hints améliorés
**Status** : En cours (Claude en train de réfléchir)
**Attendu** : Durée plus proche de 30 min

### Test 2 : Port Rust
**Status** : À faire
**Cible** : Output duration à ±1% de expected (1806s ± 18s)

### Test 3 : Validation sur autre vidéo
**Status** : À faire
**Objectif** : Vérifier que la solution généralise

## Métriques de succès

| Métrique | Avant | Cible | Après |
|----------|-------|-------|-------|
| Durée output | 39.7 min | 30.1 min | ? |
| Ratio vs expected | 131.7% | 100% ± 1% | ? |
| Chunks gardés | 180/246 | ~134/246 | ? |
| False positives | 56 | <10 | ? |

## Prochaines étapes

1. ✅ Reproduire le problème
2. ✅ Identifier la cause racine
3. ✅ Créer algorithme de détection amélioré
4. 🔄 Tester avec Claude + hints améliorés
5. ⏳ Porter vers Rust
6. ⏳ Tester sur données de test
7. ⏳ Commit + push
8. ⏳ Valider sur vidéo réelle de Jeremy
