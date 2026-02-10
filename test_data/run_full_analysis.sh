#!/bin/bash
# Full analysis workflow after rendering completes

set -e

echo "==============================================="
echo "Full Analysis Workflow - Iteration 1"
echo "==============================================="
echo

# Check if output.mp4 exists and is recent
if [ ! -f "output.mp4" ]; then
    echo "ERROR: output.mp4 not found!"
    exit 1
fi

# Get file age in seconds
file_age=$(($(date +%s) - $(stat -c %Y output.mp4)))
if [ $file_age -gt 3600 ]; then
    echo "WARNING: output.mp4 is more than 1 hour old. Is this the latest render?"
    read -p "Continue anyway? (y/n) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

echo "Step 1: Quick analysis (no transcription needed)"
echo "---------------------------------------------"
python3 quick_analysis.py
echo

# Check if we should transcribe
quick_sim=$(python3 -c "import json; print(json.load(open('reports/quick_analysis.json'))['similarity_pct'])")
echo "Quick analysis similarity: $quick_sim%"
echo

if (( $(echo "$quick_sim < 98.0" | bc -l) )); then
    echo "⚠️  Similarity < 98%, transcription recommended for detailed analysis"
    read -p "Transcribe output.mp4? (y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo
        echo "Step 2: Transcribing output.mp4 (this will take ~5-10 min)..."
        echo "---------------------------------------------"
        python3 transcribe_output.py
        
        echo
        echo "Step 3: Detailed analysis with transcription"
        echo "---------------------------------------------"
        python3 analyze_transcriptions.py
        
        echo
        echo "Step 4: Deep dive analysis"
        echo "---------------------------------------------"
        python3 deep_analysis.py
    fi
else
    echo "✅ Similarity >= 98%, transcription optional"
fi

echo
echo "==============================================="
echo "Analysis Complete!"
echo "==============================================="
echo
echo "Reports generated:"
echo "  - reports/quick_analysis.json (segment-based estimate)"
if [ -f "output_transcription.json" ]; then
    echo "  - output_transcription.json (full transcription)"
    echo "  - reports/detailed_analysis.json (word-level comparison)"
fi
echo
echo "Next steps:"
if (( $(echo "$quick_sim >= 99.0" | bc -l) )); then
    echo "  🎉 TARGET REACHED! Similarity >= 99%"
    echo "  → Git commit and push"
    echo "  → Manual review of output.mp4"
elif (( $(echo "$quick_sim >= 98.0" | bc -l) )); then
    echo "  🟡 Very close! ($quick_sim%)"
    echo "  → Consider 1 more iteration with v2 detection"
    echo "  → Or manual review to verify quality"
else
    echo "  ❌ Need iteration 2 (${quick_sim}% < 99%)"
    echo "  → Review remaining issues"
    echo "  → Apply iteration 2 improvements"
fi
