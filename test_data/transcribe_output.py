#!/usr/bin/env python3
"""Transcribe output.mp4 using AssemblyAI"""
import os
import sys
import json
import time
import subprocess
import requests

API_KEY = "bcc62b3ebf68498d9077adbc67705705"
HEADERS = {"authorization": API_KEY}
BASE_URL = "https://api.assemblyai.com/v2"

def extract_audio(video_path, audio_path):
    """Extract audio from video"""
    print(f"Extracting audio from {video_path}...")
    cmd = ["ffmpeg", "-y", "-i", video_path, "-vn", "-acodec", "libmp3lame", "-q:a", "2", audio_path]
    subprocess.run(cmd, capture_output=True, check=True)
    print(f"Audio saved to {audio_path}")

def upload_audio(audio_path):
    """Upload audio to AssemblyAI"""
    print(f"Uploading {audio_path} to AssemblyAI...")
    
    def read_file(path, chunk_size=5_242_880):
        with open(path, "rb") as f:
            while True:
                data = f.read(chunk_size)
                if not data:
                    break
                yield data
    
    response = requests.post(
        f"{BASE_URL}/upload",
        headers=HEADERS,
        data=read_file(audio_path)
    )
    response.raise_for_status()
    upload_url = response.json()["upload_url"]
    print(f"Upload complete: {upload_url}")
    return upload_url

def transcribe(audio_url):
    """Start transcription"""
    print("Starting transcription...")
    response = requests.post(
        f"{BASE_URL}/transcript",
        headers=HEADERS,
        json={
            "audio_url": audio_url,
            "language_code": "fr",
            "punctuate": True,
            "format_text": True,
        }
    )
    response.raise_for_status()
    transcript_id = response.json()["id"]
    print(f"Transcription started: {transcript_id}")
    return transcript_id

def poll_transcript(transcript_id):
    """Poll until transcription is complete"""
    while True:
        response = requests.get(
            f"{BASE_URL}/transcript/{transcript_id}",
            headers=HEADERS
        )
        response.raise_for_status()
        data = response.json()
        status = data["status"]
        
        if status == "completed":
            print("Transcription completed!")
            return data
        elif status == "error":
            print(f"Transcription error: {data.get('error', 'unknown')}")
            sys.exit(1)
        else:
            print(f"Status: {status}, waiting...")
            time.sleep(10)

def main():
    video_path = "output.mp4"
    audio_path = "output_audio.mp3"
    output_path = "output_transcription.json"
    
    # Step 1: Extract audio
    if not os.path.exists(audio_path):
        extract_audio(video_path, audio_path)
    else:
        print(f"Audio already exists: {audio_path}")
    
    # Step 2: Upload
    upload_url = upload_audio(audio_path)
    
    # Step 3: Transcribe
    transcript_id = transcribe(upload_url)
    
    # Step 4: Poll
    result = poll_transcript(transcript_id)
    
    # Step 5: Save
    with open(output_path, "w") as f:
        json.dump(result, f, indent=2, ensure_ascii=False)
    
    print(f"Saved to {output_path}")
    print(f"Words: {len(result.get('words', []))}")
    print(f"Duration: {result.get('audio_duration', 'N/A')}s")

if __name__ == "__main__":
    main()
