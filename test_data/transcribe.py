#!/usr/bin/env python3
"""Upload and transcribe audio files using AssemblyAI."""
import os
import sys
import json
import time
import requests
from dotenv import load_dotenv

load_dotenv('/root/.openclaw/workspace/autotrim-desktop/.env')
API_KEY = os.getenv('ASSEMBLYAI_API_KEY')
BASE_URL = 'https://api.assemblyai.com/v2'
HEADERS = {'authorization': API_KEY}

def upload_file(filepath):
    """Upload a local file to AssemblyAI and return the upload URL."""
    print(f"Uploading {filepath}...")
    with open(filepath, 'rb') as f:
        response = requests.post(
            f'{BASE_URL}/upload',
            headers=HEADERS,
            data=f
        )
    response.raise_for_status()
    upload_url = response.json()['upload_url']
    print(f"Upload complete: {upload_url}")
    return upload_url

def transcribe(upload_url, language='fr'):
    """Submit transcription job and wait for completion."""
    print(f"Submitting transcription for {upload_url}...")
    payload = {
        'audio_url': upload_url,
        'language_code': language,
        'punctuate': True,
        'format_text': True,
        'speech_models': ['universal-3-pro'],
    }
    response = requests.post(
        f'{BASE_URL}/transcript',
        headers=HEADERS,
        json=payload
    )
    if response.status_code != 200:
        print(f"Error {response.status_code}: {response.text}")
        response.raise_for_status()
    transcript_id = response.json()['id']
    print(f"Transcript ID: {transcript_id}")
    
    # Poll for completion
    while True:
        result = requests.get(
            f'{BASE_URL}/transcript/{transcript_id}',
            headers=HEADERS
        ).json()
        status = result['status']
        print(f"  Status: {status}")
        if status == 'completed':
            return result
        elif status == 'error':
            print(f"  Error: {result.get('error', 'unknown')}")
            return result
        time.sleep(10)

def main():
    filepath = sys.argv[1]
    output_json = sys.argv[2]
    
    # Check if we already have a cached result
    if os.path.exists(output_json):
        print(f"Cached transcription found at {output_json}, skipping.")
        return
    
    upload_url = upload_file(filepath)
    result = transcribe(upload_url)
    
    with open(output_json, 'w') as f:
        json.dump(result, f, indent=2, ensure_ascii=False)
    print(f"Saved to {output_json}")
    print(f"Text length: {len(result.get('text', ''))}")
    print(f"Words: {len(result.get('words', []))}")

if __name__ == '__main__':
    main()
