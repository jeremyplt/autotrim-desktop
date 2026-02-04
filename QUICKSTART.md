# AutoTrim Desktop - Quick Start Guide

## 🚀 5-Minute Setup

### Step 1: Install Prerequisites

**macOS:**
```bash
# Install Homebrew (if not already installed)
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install FFmpeg
brew install ffmpeg

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify installations
node --version   # Should show v18 or higher
cargo --version  # Should show rust version
ffmpeg -version  # Should show FFmpeg info
```

**Windows:**
```powershell
# Install Chocolatey (if not already installed)
# Run PowerShell as Administrator
Set-ExecutionPolicy Bypass -Scope Process -Force; [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072; iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))

# Install dependencies
choco install nodejs rust ffmpeg

# Verify installations
node --version
cargo --version
ffmpeg -version
```

**Linux:**
```bash
# Install dependencies
sudo apt update
sudo apt install -y nodejs npm ffmpeg build-essential curl

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify installations
node --version
cargo --version
ffmpeg -version
```

### Step 2: Set OpenAI API Key

The API key is already in `/root/.openclaw/workspace/.env`.

If you need to update it:
```bash
echo 'OPENAI_API_KEY="your-api-key-here"' > /root/.openclaw/workspace/.env
```

### Step 3: Install Dependencies

```bash
cd /root/.openclaw/workspace/autotrim-desktop
npm install
```

This will take 1-2 minutes.

### Step 4: Run the App

```bash
npm run tauri dev
```

**First run takes longer** (~2-5 minutes to compile Rust).
Subsequent runs are much faster (~10-30 seconds).

### Step 5: Test It Out

1. **Create a test video:**
   ```bash
   ./create-test-video.sh
   ```

2. **In the app:**
   - Drag `test_video_with_silences.mp4` into the window
   - Choose "Moderate" mode
   - Check both options (silences + repetitions)
   - Click "Start Processing"
   - Wait for completion (~30-60 seconds)

3. **View results:**
   - See stats (time saved, segments removed)
   - Click "Open in Finder" to see the output
   - Play `test_video_with_silences_trimmed.mp4`

---

## 🎯 Usage

### Basic Workflow

```
1. Select Video
   ↓
2. Choose Mode (Aggressive/Moderate/Conservative)
   ↓
3. Select Options (Remove silences/repetitions)
   ↓
4. Start Processing
   ↓
5. Wait for completion
   ↓
6. View results & open output
```

### Processing Modes

| Mode | Best For | Trimming Level |
|------|----------|----------------|
| **Aggressive** | Vlogs, casual content | Maximum |
| **Moderate** | General use | Balanced ⭐ |
| **Conservative** | Important content | Minimal |

### Expected Processing Time

| Video Length | File Size | Time |
|--------------|-----------|------|
| 5 minutes | ~100MB | 1-2 min |
| 30 minutes | ~500MB | 5-10 min |
| 1 hour | ~1GB | 10-20 min |
| 2 hours | ~20GB | 30-60 min |

---

## 🐛 Common Issues

### "FFmpeg Not Found"

**Fix:**
```bash
# macOS
brew install ffmpeg

# Verify
ffmpeg -version
```

### "OpenAI API Key Not Found"

**Fix:**
```bash
# Check .env file exists
cat /root/.openclaw/workspace/.env

# If not, create it
echo 'OPENAI_API_KEY="your-key"' > /root/.openclaw/workspace/.env
```

### Rust Compilation Fails

**Fix:**
```bash
# Update Rust
rustup update stable

# Clean and rebuild
cd src-tauri
cargo clean
cd ..
npm run tauri dev
```

### App Won't Start

**Fix:**
```bash
# Check all prerequisites
node --version    # v18+
cargo --version   # latest
ffmpeg -version   # any version

# Reinstall dependencies
rm -rf node_modules
npm install
```

---

## 📁 Output Location

Processed videos are saved in the same folder as the input:

```
/path/to/your/video.mp4
                    ↓
/path/to/your/video_trimmed.mp4
```

---

## 💰 API Costs

OpenAI Whisper API: **~$0.006 per minute** of audio

Examples:
- 5-minute video: ~$0.03
- 30-minute video: ~$0.18
- 1-hour video: ~$0.36
- 2-hour video: ~$0.72

---

## 🎓 Tips

1. **Start small**: Test with short videos first
2. **Use Moderate**: Best balance of safety and efficiency
3. **Check silences only**: If you don't need repetition removal, uncheck it to save API costs
4. **Disk space**: Ensure you have 2x the video file size free
5. **Be patient**: Large videos take time, but progress is shown

---

## 📚 Full Documentation

- **README.md**: Complete user guide
- **DEVELOPMENT.md**: Technical documentation
- **BUILD_REPORT.md**: What was built and how

---

## ✅ That's It!

You're ready to trim videos. Enjoy! 🎬✨

---

**Questions?** Check the README.md or DEVELOPMENT.md for detailed info.
