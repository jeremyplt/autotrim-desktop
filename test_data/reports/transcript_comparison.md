# Transcript Comparison Report

## Summary

| Metric | Value |
|--------|-------|
| Expected word count | 6568 |
| Output word count | 6478 |
| Matching words | 6274 |
| Match % (of expected) | 95.5% |
| Match % (of output) | 96.9% |
| Words missing from output | 128 |
| Extra words in output | 36 |
| Words in replacements (expected side) | 166 |
| Words in replacements (output side) | 168 |
| ASR noise replacements | 97 (120 exp words) |
| Content difference replacements | 31 (46 exp words) |

### Overall Assessment

- **Content error rate**: 3.2% (210 words)
- **ASR noise rate**: 1.8% (120 words)
- **True content match**: 96.8%

✅ **Content match is GOOD** (< 5% error). Differences are primarily ASR transcription noise.

## Missing Content (in expected, not in output)

Total: 70 gaps, 128 words

### Significant gaps (>3 words)

**Gap 1** (4 words) at expected timestamp 13:31-13:34:
> YouTube CLO Test Bot.

**Gap 2** (4 words) at expected timestamp 17:33-17:33:
> Et une fois que

**Gap 3** (4 words) at expected timestamp 18:47-18:48:
> qui sont très intéressantes,

**Gap 4** (4 words) at expected timestamp 21:26-21:26:
> donc text to speech,

**Gap 5** (6 words) at expected timestamp 21:33-21:34:
> Ça, c'est le speech to text.

**Gap 6** (4 words) at expected timestamp 21:37-21:38:
> le text to speech,

**Gap 7** (4 words) at expected timestamp 24:22-24:23:
> donc une story card.

**Gap 8** (4 words) at expected timestamp 26:26-26:26:
> qui est le titre

**Gap 9** (4 words) at expected timestamp 29:07-29:07:
> sur la, le bouton

**Gap 10** (5 words) at expected timestamp 29:43-29:44:
> d'excuses. Il y a pas

## Extra Content (in output, not in expected)

Total: 29 insertions, 36 words

### Significant insertions (>3 words)

**Insert 1** (4 words) at output timestamp 27:23-27:24:
> Donc là quand je

## Content Differences (replacements)

Total replacements: 128
- ASR noise (similar words): 97
- Content differences: 31

### Significant content differences (>3 words)

**Diff 1** (exp: 2 words, out: 6 words)
- Expected timestamp: 03:28-03:28
- Output timestamp: 03:28-03:29
- Expected: > dis c'est
- Output: > me suis dit que ce n'est

**Diff 2** (exp: 1 words, out: 8 words)
- Expected timestamp: 13:39-13:39
- Output timestamp: 13:49-13:52
- Expected: > OpenClo.
- Output: > OpenCode. Ici tu vas devoir mettre ton propre...

**Diff 3** (exp: 5 words, out: 1 words)
- Expected timestamp: 18:48-18:50
- Output timestamp: 19:10-19:10
- Expected: > les cron jobs, c'est-à-dire les
- Output: > l'écrire,

**Diff 4** (exp: 4 words, out: 1 words)
- Expected timestamp: 19:47-19:49
- Output timestamp: 20:08-20:09
- Expected: > ChatGPT ou sur Cloud.
- Output: > chat.gpt.

## ASR Noise Examples (first 20)

- `J'utilise` → `Je n'utilise` (exp 00:06)
- `j'ai` → `je n'ai` (exp 00:08)
- `OpenClo.` → `OpenCloud.` (exp 00:13)
- `Cloud Code,` → `CloudCode,` (exp 00:40)
- `Cloud Code,` → `CloudCode,` (exp 00:51)
- `Claude` → `Cloud` (exp 01:26)
- `Claude` → `Cloud` (exp 01:31)
- `bah c'est` → `ce n'est` (exp 01:34)
- `Et Claude` → `Cloud` (exp 01:35)
- `Claude` → `Cloud` (exp 01:44)
- `Claude` → `Cloud` (exp 02:32)
- `c'est` → `ce n'est` (exp 02:34)
- `Claude` → `Cloud` (exp 02:43)
- `d'itérations maximale,` → `d'interactions maximal.` (exp 02:46)
- `Claude` → `Cloud` (exp 02:55)
- `Ralfloop,` → `sur RalphLoop,` (exp 03:17)
- `Ralfloop,` → `RalphLoop,` (exp 03:20)
- `Ralfloop,` → `RalphLoop,` (exp 03:25)
- `maximales` → `maximale` (exp 03:54)
- `application-là,` → `application là` (exp 04:10)

## Timeline Analysis

Checking for large time gaps or out-of-order content...

✅ No backward time jumps — content appears to be in correct order.

## Problem Areas by Timestamp

| Type | Time | Words | Preview |
|------|------|-------|---------|
| DIFF | 03:28-03:28 | 8 | EXP: dis c'est → OUT: me suis dit que ce n'est |
| MISSING | 13:31-13:34 | 4 | YouTube CLO Test Bot. |
| DIFF | 13:39-13:39 | 9 | EXP: OpenClo. → OUT: OpenCode. Ici tu vas devoir mettre ton propre... |
| MISSING | 17:33-17:33 | 4 | Et une fois que |
| MISSING | 18:47-18:48 | 4 | qui sont très intéressantes, |
| DIFF | 18:48-18:50 | 6 | EXP: les cron jobs, c'est-à-dire les → OUT: l'écrire, |
| DIFF | 19:47-19:49 | 5 | EXP: ChatGPT ou sur Cloud. → OUT: chat.gpt. |
| MISSING | 21:26-21:26 | 4 | donc text to speech, |
| MISSING | 21:33-21:34 | 6 | Ça, c'est le speech to text. |
| MISSING | 21:37-21:38 | 4 | le text to speech, |
| MISSING | 24:22-24:23 | 4 | donc une story card. |
| MISSING | 26:26-26:26 | 4 | qui est le titre |
| EXTRA | 27:23-27:24 | 4 | Donc là quand je |
| MISSING | 29:07-29:07 | 4 | sur la, le bouton |
| MISSING | 29:43-29:44 | 5 | d'excuses. Il y a pas |

## Root Cause Analysis

The 3.2% content error rate is within acceptable bounds. The differences are primarily due to:

1. **ASR transcription variability**: Different audio encoding/quality leads to slightly different word recognition (97 instances)
2. **Minor word boundary differences**: Small insertions/deletions at phrase boundaries
3. **Small content variations**: 10 missing segments and 1 extra segments, which may represent minor timing differences in the cut points
