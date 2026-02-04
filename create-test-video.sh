#!/bin/bash

# AutoTrim Test Video Generator
# Creates test videos with silences and repetitive audio for testing

set -e

if ! command -v ffmpeg &> /dev/null; then
    echo "❌ FFmpeg is not installed. Please install it first."
    echo ""
    echo "  macOS:   brew install ffmpeg"
    echo "  Windows: choco install ffmpeg"
    echo "  Linux:   sudo apt install ffmpeg"
    exit 1
fi

echo "🎬 Creating test video with silences and speech..."

# Create a test video with:
# 1. Color bars visual
# 2. Alternating segments of tone and silence
# 3. Some "repetitive" patterns in audio

# Duration settings
TONE_DURATION=3
SILENCE_DURATION=2
TOTAL_SEGMENTS=5

echo "📹 Generating video with color bars..."
echo "🔊 Creating audio pattern: (${TONE_DURATION}s tone + ${SILENCE_DURATION}s silence) × ${TOTAL_SEGMENTS}"

# Calculate total duration
TOTAL_DURATION=$((($TONE_DURATION + $SILENCE_DURATION) * $TOTAL_SEGMENTS))

echo "⏱️  Total duration: ${TOTAL_DURATION} seconds"

# Create the filter complex string
FILTER="color=c=blue:s=1920x1080:d=${TONE_DURATION}[v0];"
FILTER+="sine=frequency=440:duration=${TONE_DURATION}[a0];"
FILTER+="color=c=black:s=1920x1080:d=${SILENCE_DURATION}[v1];"
FILTER+="anullsrc=duration=${SILENCE_DURATION}[a1];"

# Build concatenation string
VIDEO_CONCAT=""
AUDIO_CONCAT=""

for i in $(seq 0 $((TOTAL_SEGMENTS - 1))); do
    VIDEO_CONCAT+="[v0][v1]"
    AUDIO_CONCAT+="[a0][a1]"
done

FILTER+="${VIDEO_CONCAT}concat=n=$((TOTAL_SEGMENTS * 2)):v=1:a=0[vout];"
FILTER+="${AUDIO_CONCAT}concat=n=$((TOTAL_SEGMENTS * 2)):v=0:a=1[aout]"

# Generate the test video
ffmpeg -f lavfi -i color=c=blue:s=1920x1080:d=${TONE_DURATION} \
       -f lavfi -i sine=frequency=440:duration=${TONE_DURATION} \
       -f lavfi -i color=c=black:s=1920x1080:d=${SILENCE_DURATION} \
       -f lavfi -i anullsrc=duration=${SILENCE_DURATION} \
       -filter_complex "${FILTER}" \
       -map "[vout]" -map "[aout]" \
       -c:v libx264 -preset fast -crf 23 \
       -c:a aac -b:a 128k \
       -t ${TOTAL_DURATION} \
       -y test_video_with_silences.mp4

echo "✅ Test video created: test_video_with_silences.mp4"
echo ""
echo "📊 Video stats:"
echo "   Duration: ${TOTAL_DURATION} seconds"
echo "   Tone segments: ${TOTAL_SEGMENTS} × ${TONE_DURATION}s = $((TOTAL_SEGMENTS * TONE_DURATION))s"
echo "   Silent segments: ${TOTAL_SEGMENTS} × ${SILENCE_DURATION}s = $((TOTAL_SEGMENTS * SILENCE_DURATION))s"
echo "   Expected after trimming: ~$((TOTAL_SEGMENTS * TONE_DURATION))s"
echo ""
echo "🎯 To test with AutoTrim:"
echo "   1. npm run tauri dev"
echo "   2. Select test_video_with_silences.mp4"
echo "   3. Choose 'Moderate' mode"
echo "   4. Enable 'Remove silences'"
echo "   5. Start processing"
echo ""
echo "   Expected result: Video should be ~$((TOTAL_SEGMENTS * TONE_DURATION))s (silences removed)"

# Create a simple video file for basic testing
echo ""
echo "📹 Creating simple 10-second test video..."

ffmpeg -f lavfi -i testsrc=duration=10:size=1920x1080:rate=30 \
       -f lavfi -i sine=frequency=1000:duration=10 \
       -c:v libx264 -preset fast -crf 23 \
       -c:a aac -b:a 128k \
       -y test_video_simple.mp4

echo "✅ Simple test video created: test_video_simple.mp4"
echo ""
echo "🎉 All test videos created successfully!"
