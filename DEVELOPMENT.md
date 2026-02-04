# Development Guide

## 🏗️ Architecture

### Frontend (React + TypeScript)

The frontend follows a component-based architecture with clear separation of concerns:

```
App.tsx (State Management)
├── VideoSelector (File Input)
├── SettingsPanel (Configuration)
├── ProcessingView (Progress Display)
└── ResultView (Stats & Actions)
```

**State Flow:**
1. User selects video → `VideoInfo` stored
2. User configures settings → `ProcessingSettings` stored
3. User starts processing → `jobId` created
4. Progress polling begins → `Progress` updates
5. Processing completes → `ProcessingResult` displayed

### Backend (Rust)

The backend is modular with clear responsibilities:

- **commands.rs**: Tauri command handlers (bridge to frontend)
- **ffmpeg.rs**: FFmpeg wrapper for video operations
- **transcription.rs**: Whisper API client and phrase detection
- **processor.rs**: Main processing orchestration

**Processing Pipeline:**
```
Video File
    ↓
Extract Audio (FFmpeg)
    ↓
Transcribe (Whisper API) ← Parallel → Detect Silences (FFmpeg)
    ↓                                      ↓
Segment into Phrases                   Silence Segments
    ↓                                      ↓
Detect Repetitions                     Merge with Repetitions
    ↓                                      ↓
    └──────────→ Combined Segments ←───────┘
                        ↓
                Calculate Keep Segments
                        ↓
                Render Final Video (FFmpeg)
```

## 🔧 Key Technical Decisions

### Why Tauri Instead of Electron?

- **Size**: 15MB vs 200MB+ for Electron
- **Performance**: Native Rust backend, no Node.js overhead
- **Security**: Better sandboxing and permission model
- **Resources**: Lower memory and CPU usage

### FFmpeg Integration

**Why not bundle FFmpeg?**
- Legal/licensing considerations
- Platform-specific binaries (increases bundle size)
- Users likely already have it installed
- Easy installation via package managers

**Future**: Could bundle using `ffmpeg-static` crate for better UX

### Whisper API vs Local

**Current**: OpenAI Whisper API
- ✅ Fast, no local setup
- ✅ Word-level timestamps
- ✅ High accuracy
- ❌ Costs ~$0.006/minute
- ❌ Requires internet
- ❌ Privacy concerns (audio leaves machine)

**Future**: whisper.cpp local processing
- ✅ Free, offline
- ✅ Privacy-friendly
- ❌ Slower on CPU
- ❌ Requires model download (~1GB)

### Segment Detection Algorithm

**Silence Detection:**
- Uses FFmpeg's `silencedetect` filter
- Configurable threshold (dB) and min duration
- Outputs start/end timestamps
- Regex parsing of FFmpeg stderr

**Repetition Detection:**
1. Segment transcript into phrases (by punctuation/pauses)
2. Compare each phrase with all following phrases
3. Calculate word-based similarity score
4. If similarity > threshold, mark earlier occurrence for removal
5. **Keep last occurrence** (most refined version)

**Why last instead of first?**
- People often correct themselves
- Last take is usually the keeper
- Matches natural editing workflow

### Video Rendering

**Challenge**: Concatenating non-contiguous segments

**Solution**: FFmpeg filter_complex
```bash
# For each segment to keep:
[0:v]trim=start=X:end=Y,setpts=PTS-STARTPTS[vN]
[0:a]atrim=start=X:end=Y,asetpts=PTS-STARTPTS[aN]

# Then concatenate all:
[v0][a0][v1][a1]...[vN][aN]concat=n=N:v=1:a=1[outv][outa]
```

**Codec Choices:**
- Video: libx264, preset medium, CRF 23 (balance of quality/size)
- Audio: AAC, 128k bitrate

**Future Optimization:**
- Use `-c copy` when possible (no re-encode)
- Detect if segments are keyframe-aligned
- GPU-accelerated encoding

## 🎨 Design Implementation

### Color System

All colors defined in multiple places for consistency:
- `tailwind.config.js` - TailwindCSS utilities
- `src/styles/globals.css` - CSS variables
- Component inline styles where needed

### Animations

Using Framer Motion for:
- **Page transitions**: fade in/out with AnimatePresence
- **Component entrance**: staggered delays
- **Hover effects**: scale, opacity
- **Progress bar**: smooth width transitions

**Performance**: Animations use CSS transforms (GPU-accelerated)

### Responsive Design

While primarily desktop, the UI scales reasonably:
- Min width: 900px
- Max width: 4xl (1200px) centered
- Components use flexbox for flexibility

## 🧪 Testing Strategy

### Unit Tests (Not Yet Implemented)

**Frontend:**
```typescript
// utils.test.ts
test('formatBytes', () => {
  expect(formatBytes(1024)).toBe('1 KB')
  expect(formatBytes(1048576)).toBe('1 MB')
})
```

**Backend:**
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_silence_output() {
        // Test FFmpeg output parsing
    }
}
```

### Integration Tests

**Manual Checklist** (see README.md)

**Automated** (future):
- Create test videos programmatically
- Run processing with known inputs
- Verify outputs match expected durations
- Check file integrity

## 🐛 Known Issues & Limitations

### Current Limitations

1. **No Undo**: Once processing starts, can only cancel (no resume)
2. **Single Processing**: Can't queue multiple videos
3. **Memory**: Large videos (50GB+) may cause high memory usage during transcription
4. **FFmpeg Dependency**: User must install separately
5. **No Preview**: Can't preview segments before rendering

### Edge Cases

1. **Video with no audio**: Will fail (need audio for transcription)
2. **Silent videos**: Remove silences would trim everything
3. **Corrupted videos**: FFmpeg may crash, job will hang
4. **Very long videos**: API timeout possible (>3 hours)

### Future Improvements

1. **Better Error Handling**:
   - Specific error messages
   - Retry logic for API failures
   - Graceful degradation

2. **Performance Optimizations**:
   - Parallel processing multiple jobs
   - GPU acceleration
   - Smart caching (don't re-transcribe same audio)

3. **UX Enhancements**:
   - Preview timeline with segments highlighted
   - Manual segment adjustment
   - Batch processing
   - Preset saving

4. **Advanced Features**:
   - Custom word filters (remove specific words)
   - Scene detection integration
   - Audio normalization
   - Chapter markers

## 🔒 Security Considerations

### API Key Storage

**Current**: Read from .env file or environment variable
- ❌ Not encrypted at rest
- ❌ No OS keychain integration

**Should Add**:
- OS keychain storage (keytar, keyring-rs)
- In-app API key input with secure storage
- Warn if API key in environment variables

### File Access

**Current**: Full filesystem access via Tauri
- User can select any video file
- Output written to same directory

**Good**:
- User explicitly selects files (no arbitrary access)
- No network access except Whisper API
- No analytics/telemetry

### Content Security Policy

**Current**: CSP disabled (`"csp": null`)
- Required for some Tauri features
- Not a concern for single-user desktop app

**Production**:
- Could enable strict CSP
- Inline scripts use nonces

## 📊 Performance Profiling

### Bottlenecks

1. **Whisper API**: 10-30s for typical video
2. **FFmpeg Rendering**: 50-80% of total time
3. **Silence Detection**: Usually <10s
4. **Repetition Detection**: Negligible (in-memory)

### Optimization Opportunities

1. **Parallel API Calls**: Split long audio into chunks
2. **Streaming**: Start rendering while still detecting
3. **Smart Encoding**: Use `-c copy` when possible
4. **Caching**: Cache transcriptions (hash-based)

## 🛠️ Development Workflow

### Hot Reload

Vite provides instant frontend hot reload. Rust changes require recompilation (~5-30s depending on what changed).

### Debugging

**Frontend**:
```typescript
console.log('Debug:', data)
```
DevTools open automatically in debug mode.

**Backend**:
```rust
eprintln!("Debug: {:?}", data);
```
Output appears in terminal running `tauri dev`.

### Building for Release

```bash
# Full production build
npm run tauri build

# Debug build (faster, larger)
npm run tauri build -- --debug
```

## 📚 Dependencies

### Frontend

- **React 19**: Latest with improved performance
- **Framer Motion**: 60fps animations
- **Lucide React**: 1000+ icons, tree-shakeable
- **TailwindCSS**: Utility-first CSS
- **shadcn/ui**: Copy-paste components (not npm package)

### Backend

- **Tauri 2**: Latest with better Windows support
- **Tokio**: Async runtime for Rust
- **Reqwest**: HTTP client for Whisper API
- **Serde**: Serialization/deserialization
- **Regex**: FFmpeg output parsing

## 🎓 Learning Resources

- [Tauri Docs](https://tauri.app)
- [FFmpeg Filters Guide](https://ffmpeg.org/ffmpeg-filters.html)
- [Whisper API Reference](https://platform.openai.com/docs/guides/speech-to-text)
- [React + TypeScript Patterns](https://react-typescript-cheatsheet.netlify.app/)

## 🤝 Contributing Guidelines

### Code Style

**TypeScript**:
- Use functional components with hooks
- Prefer `const` over `let`
- Use TypeScript strict mode
- No `any` types (use `unknown` if needed)

**Rust**:
- Follow `rustfmt` defaults
- Use `clippy` lints
- Prefer Result<T, E> over panicking
- Document public APIs

### Commit Messages

```
feat: add batch processing support
fix: resolve memory leak in large file processing
docs: update installation instructions
refactor: simplify segment merging logic
test: add unit tests for utils
```

### Pull Request Process

1. Create feature branch from `main`
2. Implement changes with tests
3. Update documentation (README, inline docs)
4. Run linters: `npm run lint`, `cargo clippy`
5. Test manually with multiple video types
6. Submit PR with description of changes

---

**Happy coding! 🚀**
