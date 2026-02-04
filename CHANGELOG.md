# Changelog

All notable changes to AutoTrim Desktop will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-02-04

### Added
- Initial release of AutoTrim Desktop
- Video file selection via drag & drop or file picker
- Three processing modes: Aggressive, Moderate, Conservative
- Automatic silence detection using FFmpeg
- Automatic repetition detection using OpenAI Whisper API
- Real-time progress tracking with ETA
- Processing statistics display (time saved, segments removed)
- "Open in Finder/Explorer" functionality
- Premium dark mode UI inspired by Linear.app and Raycast
- Support for MP4, MOV, MKV, AVI, WEBM formats
- Cross-platform support (macOS, Windows, Linux)
- No file size limits (local processing)
- FFmpeg installation check on startup

### Technical Details
- Built with Tauri 2 (Rust + React + TypeScript)
- Frontend: React 19, Vite, TailwindCSS, Framer Motion, shadcn/ui
- Backend: Rust with Tokio async runtime
- Video processing: FFmpeg with filter_complex
- Transcription: OpenAI Whisper API with word-level timestamps
- Custom repetition detection algorithm (keeps last occurrence)
- Segment merging algorithm for efficient rendering

### Design
- Custom color scheme with exact hex values
- Inter font for all typography
- Smooth animations with Framer Motion
- Responsive component layouts
- Premium UI components with shadcn/ui
- Consistent spacing and border radius
- Subtle hover states and transitions

### Known Issues
- Requires FFmpeg to be pre-installed
- Requires OpenAI API key for transcription
- Single video processing only (no batch mode)
- No preview of segments before rendering
- No undo/resume functionality

### Future Improvements
- Local Whisper.cpp integration (no API costs)
- Batch processing support
- Segment preview timeline
- GPU-accelerated rendering
- Custom output format selection
- Preset management
- Advanced settings UI

---

## [Unreleased]

### Planned Features
- [ ] Local transcription (whisper.cpp)
- [ ] Batch processing
- [ ] Timeline preview
- [ ] GPU acceleration
- [ ] Custom presets
- [ ] Advanced settings panel
- [ ] Export trim log
- [ ] Undo/redo support
- [ ] Multiple language support
- [ ] Cloud storage integration
- [ ] Command-line interface

### Under Consideration
- [ ] Video format conversion
- [ ] Audio enhancement
- [ ] Chapter marker generation
- [ ] Social media export presets
- [ ] Collaborative editing
- [ ] Plugin system

---

**Legend:**
- `Added` for new features
- `Changed` for changes in existing functionality
- `Deprecated` for soon-to-be removed features
- `Removed` for now removed features
- `Fixed` for any bug fixes
- `Security` for vulnerability fixes
