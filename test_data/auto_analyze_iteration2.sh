#!/bin/bash
# Auto-analysis for iteration 2 once rendering completes

echo "Waiting for autotrim.py to complete..."
while pgrep -f "python3 autotrim.py" > /dev/null; do
    sleep 10
done

echo "✅ Rendering complete!"
echo

# Backup iteration 2 output
if [ -f "output.mp4" ]; then
    cp output.mp4 output_iteration2.mp4
    echo "✅ Backed up to output_iteration2.mp4"
fi

# Run quick analysis
echo "Running quick analysis..."
python3 quick_analysis.py

# Get similarity
sim=$(python3 -c "import json; print(json.load(open('reports/quick_analysis.json'))['similarity_pct'])")
echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "ITERATION 2 RESULTS"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Similarity: $sim%"

if (( $(echo "$sim >= 99.0" | bc -l) )); then
    echo "🎉 SUCCESS! Reached 99%+ target!"
    echo
    echo "Next steps:"
    echo "  1. Manual review of output.mp4"
    echo "  2. Optional: Transcribe for verification"
    echo "  3. Commit iteration 2"
    echo "  4. Push to GitHub"
elif (( $(echo "$sim >= 98.0" | bc -l) )); then
    echo "🟡 Very close! ($sim%)"
    echo
    echo "Consider:"
    echo "  1. Manual review to verify quality"
    echo "  2. If quality is good, may be sufficient"
    echo "  3. Or run iteration 3 for final 1%"
else
    echo "⚠️ Need iteration 3 ($sim%)"
    echo
    echo "Debug steps:"
    echo "  1. Review removed segments"
    echo "  2. Check if retake detection was too aggressive"
    echo "  3. Adjust thresholds"
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo
echo "Detailed report: reports/quick_analysis.json"
