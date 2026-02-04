# AutoTrim Desktop

A premium desktop application for automatically removing silences and repetitions from videos. Built with Tauri (Rust + React + TypeScript).

## ✨ Features

- 🎬 **Smart Video Processing**: Automatically detect and remove silences and repetitions
- 🎨 **Premium Design**: Linear.app/Raycast-level UI with dark mode
- 🚀 **Native Performance**: Built with Rust for maximum speed
- 💾 **No Size Limits**: Process large 4K videos locally
- 🔊 **AI-Powered**: Uses OpenAI Whisper for accurate transcription
- 📊 **Progress Tracking**: Real-time progress with ETA

## 🏗️ Tech Stack

**Frontend:**
- React 19 with TypeScript
- Vite for blazing fast development
- TailwindCSS with custom design system
- Framer Motion for smooth animations
- shadcn/ui components
- Lucide React icons

**Backend:**
- Rust with Tauri 2
- FFmpeg for video processing
- OpenAI Whisper API for transcription
- Async processing with Tokio

## 📋 Prerequisites

### Required

1. **Node.js** (v18 or higher)
   ```bash
   node --version
   ```

2. **Rust** (latest stable)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

3. **FFmpeg** (must be in PATH)
   ```bash
   # macOS
   brew install ffmpeg
   
   # Windows
   choco install ffmpeg
   
   # Linux
   sudo apt install ffmpeg
   ```

4. **OpenAI API Key**
   - Create a file: `/root/.openclaw/workspace/.env`
   - Add: `OPENAI_API_KEY="your-key-here"`
   - Or set environment variable: `export OPENAI_API_KEY="your-key-here"`

### macOS Specific

```bash
xcode-select --install
```

### Linux Specific

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

### Windows Specific

- Install [Microsoft Visual C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
- Install [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)

## 🚀 Getting Started

### 1. Install Dependencies

```bash
cd /root/.openclaw/workspace/autotrim-desktop
npm install
```

### 2. Set Up OpenAI API Key

Create or edit `/root/.openclaw/workspace/.env`:
```env
OPENAI_API_KEY="sk-proj-your-api-key-here"
```

### 3. Run in Development Mode

```bash
npm run tauri dev
```

This will:
- Start the Vite dev server
- Compile the Rust backend
- Launch the app with hot-reload

### 4. Build for Production

```bash
npm run tauri build
```

Output locations:
- **macOS**: `src-tauri/target/release/bundle/dmg/AutoTrim_0.1.0_*.dmg`
- **Windows**: `src-tauri/target/release/bundle/msi/AutoTrim_0.1.0_*.msi`
- **Linux**: `src-tauri/target/release/bundle/deb/autotrim_0.1.0_*.deb`

## 🎮 How to Use

1. **Launch the app**
2. **Select a video**:
   - Drag and drop a video file
   - Or click to browse (supports MP4, MOV, MKV, AVI, WEBM)
3. **Choose processing mode**:
   - **Aggressive**: Maximum trimming, fastest results
   - **Moderate**: Balanced approach (recommended)
   - **Conservative**: Minimal trimming, safest option
4. **Select options**:
   - ✅ Remove silences
   - ✅ Remove repetitions (keeps last occurrence)
5. **Click "Start Processing"**
6. **Wait for completion** (progress shown with ETA)
7. **View results** and open the output folder

## 🎨 Design System

### Colors

```css
--bg-primary: #0A0A0B      /* Main background */
--bg-secondary: #141415     /* Cards, panels */
--bg-tertiary: #1C1C1E      /* Hover states */
--border: #2A2A2D           /* Subtle borders */
--text-primary: #FAFAFA     /* Main text */
--text-secondary: #A1A1A6   /* Secondary text */
--accent: #6366F1           /* Indigo accent */
--success: #22C55E          /* Green success */
--error: #EF4444            /* Red error */
```

### Typography

- **Font**: Inter (Google Fonts)
- **Headings**: font-semibold, tracking-tight
- **Body**: font-normal, text-sm/text-base

## 🔧 Project Structure

```
autotrim-desktop/
├── src/                      # Frontend (React + TypeScript)
│   ├── components/
│   │   ├── ui/              # shadcn/ui components
│   │   │   ├── button.tsx
│   │   │   ├── card.tsx
│   │   │   └── progress.tsx
│   │   ├── VideoSelector.tsx
│   │   ├── SettingsPanel.tsx
│   │   ├── ProcessingView.tsx
│   │   └── ResultView.tsx
│   ├── lib/
│   │   └── utils.ts         # Utility functions
│   ├── styles/
│   │   └── globals.css      # Global styles
│   ├── App.tsx              # Main app component
│   └── main.tsx             # Entry point
├── src-tauri/               # Backend (Rust)
│   ├── src/
│   │   ├── main.rs          # Entry point
│   │   ├── lib.rs           # Library setup
│   │   ├── commands.rs      # Tauri commands
│   │   ├── ffmpeg.rs        # FFmpeg wrapper
│   │   ├── transcription.rs # Whisper API
│   │   └── processor.rs     # Main processing logic
│   ├── Cargo.toml           # Rust dependencies
│   └── tauri.conf.json      # Tauri config
├── package.json
├── vite.config.ts
├── tailwind.config.js
└── tsconfig.json
```

## 🧪 Testing

### Create a Test Video

```bash
# Install FFmpeg first
# Create a 30-second test video with some silent parts
ffmpeg -f lavfi -i testsrc=duration=30:size=1920x1080:rate=30 \
  -f lavfi -i sine=frequency=1000:duration=5 \
  -f lavfi -i anullsrc=duration=5 \
  -filter_complex "[1:a][2:a]concat=n=2:v=0:a=1[a]" \
  -map 0:v -map "[a]" -c:v libx264 -c:a aac \
  test_video.mp4
```

### Manual Testing Checklist

- [ ] App launches without errors
- [ ] FFmpeg check works correctly
- [ ] Video file selection (drag & drop)
- [ ] Video file selection (file picker)
- [ ] All three modes selectable
- [ ] Options checkboxes toggle
- [ ] Processing starts successfully
- [ ] Progress updates in real-time
- [ ] ETA displays correctly
- [ ] Processing can be canceled
- [ ] Final video is created
- [ ] Stats are accurate
- [ ] "Open in Finder/Explorer" works
- [ ] "Process Another" resets the app

## 🐛 Troubleshooting

### FFmpeg Not Found

**Error**: "FFmpeg Not Found" on startup

**Solution**:
```bash
# Verify FFmpeg is installed
ffmpeg -version

# If not found, install it:
# macOS: brew install ffmpeg
# Windows: choco install ffmpeg
# Linux: sudo apt install ffmpeg

# Ensure it's in your PATH
echo $PATH  # Should include FFmpeg location
```

### OpenAI API Key Error

**Error**: "OpenAI API key not found"

**Solution**:
1. Create `/root/.openclaw/workspace/.env`
2. Add: `OPENAI_API_KEY="your-key-here"`
3. Restart the app

### Build Errors

**Error**: Rust compilation fails

**Solution**:
```bash
# Update Rust
rustup update stable

# Clean build
cd src-tauri
cargo clean
cargo build
```

### Video Processing Fails

**Possible causes**:
- Corrupt video file
- Unsupported codec
- Insufficient disk space
- FFmpeg crash

**Solution**:
- Check console logs for errors
- Try a different video file
- Ensure you have enough free space
- Verify FFmpeg works: `ffmpeg -version`

## 📊 Performance

- **Small videos** (< 100MB): ~1-2 minutes
- **Medium videos** (100MB - 1GB): ~3-10 minutes
- **Large videos** (1GB+): ~10-30 minutes
- **4K 2-hour videos** (20-50GB): ~1-2 hours

*Times vary based on CPU, video complexity, and settings chosen.*

## 🔒 Privacy & Security

- **100% Local Processing**: All video processing happens on your machine
- **API Usage**: Only audio is sent to OpenAI Whisper API for transcription
- **No Tracking**: No analytics, no telemetry, no data collection
- **Your Data**: Videos never leave your computer (except audio for transcription)

## 📝 License

MIT License - See LICENSE file for details

## 🤝 Contributing

This is currently a private project. If you'd like to contribute, please contact the maintainer.

## 🎯 Roadmap

- [ ] Local Whisper.cpp integration (no API costs)
- [ ] Batch processing multiple videos
- [ ] Custom output format selection
- [ ] Advanced silence detection settings UI
- [ ] GPU acceleration for rendering
- [ ] Preview before/after segments
- [ ] Export trim log/timestamps

## 💡 Tips

1. **Start with Moderate mode** for best balance
2. **Conservative mode** for important content
3. **Aggressive mode** for casual vlogs/screencasts
4. **Large files**: Ensure plenty of free disk space (2x video size)
5. **API costs**: ~$0.006 per minute of audio transcribed

## 📧 Support

For issues or questions, check the console logs and refer to the troubleshooting section above.

---

**Made with ❤️ using Tauri, React, and Rust**
