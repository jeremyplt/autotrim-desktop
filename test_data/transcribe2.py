#!/usr/bin/env python3
"""Upload and transcribe audio files using AssemblyAI. Flush-friendly."""
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

def log(msg):
    print(msg, flush=True)

def upload_file(filepath):
    log(f"Uploading {filepath} ({os.path.getsize(filepath)/1e6:.1f}MB)...")
    with open(filepath, 'rb') as f:
        response = requests.post(f'{BASE_URL}/upload', headers=HEADERS, data=f)
    response.raise_for_status()
    url = response.json()['upload_url']
    log(f"Upload complete: {url}")
    return url

def submit_transcription(upload_url, language='fr'):
    payload = {
        'audio_url': upload_url,
        'language_code': language,
        'punctuate': True,
        'format_text': True,
        'speech_models': ['universal-3-pro'],
    }
    log(f"Submitting transcription...")
    response = requests.post(f'{BASE_URL}/transcript', headers=HEADERS, json=payload)
    if response.status_code != 200:
        log(f"Error {response.status_code}: {response.text}")
        sys.exit(1)
    tid = response.json()['id']
    log(f"Transcript ID: {tid}")
    return tid

def poll_transcript(tid):
    while True:
        result = requests.get(f'{BASE_URL}/transcript/{tid}', headers=HEADERS).json()
        status = result['status']
        log(f"  Status: {status}")
        if status == 'completed':
            return result
        elif status == 'error':
            log(f"  Error: {result.get('error', 'unknown')}")
            sys.exit(1)
        time.sleep(15)

def main():
    filepath = sys.argv[1]
    output_json = sys.argv[2]
    
    if os.path.exists(output_json):
        log(f"Cached transcription found at {output_json}, skipping.")
        return
    
    upload_url = upload_file(filepath)
    tid = submit_transcription(upload_url)
    result = poll_transcript(tid)
    
    with open(output_json, 'w') as f:
        json.dump(result, f, indent=2, ensure_ascii=False)
    log(f"Saved to {output_json}")
    log(f"Text length: {len(result.get('text', ''))}")
    log(f"Words: {len(result.get('words', []))}")

if __name__ == '__main__':
    main()
