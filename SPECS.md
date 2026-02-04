# AutoTrim Desktop - App Tauri

## 🎯 Objectif

Application desktop native pour:
1. Sélectionner une vidéo locale (AUCUNE limite de taille)
2. Supprimer automatiquement silences + répétitions
3. Exporter la vidéo trimmée

## 🏗️ Stack Technique

### Tauri (Framework Desktop)
- **Backend**: Rust (appels système, FFmpeg, file I/O)
- **Frontend**: React + Vite + TypeScript
- **Taille finale**: ~15MB (vs Electron 200MB+)
- **Performance**: Native, pas de overhead Node.js

### Processing
- **FFmpeg**: Installé localement ou bundlé avec l'app
- **Whisper API** (OpenAI): Pour transcription avec timestamps
- **Alternative future**: Whisper.cpp local (0 coût)

## 📁 Structure du Projet

```
autotrim-desktop/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs           # Entry point Tauri
│   │   ├── commands.rs       # Commandes exposées au frontend
│   │   ├── ffmpeg.rs         # FFmpeg wrapper
│   │   ├── transcription.rs  # Whisper API client
│   │   ├── processor.rs      # Logique de processing
│   │   └── lib.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/
│   ├── App.tsx
│   ├── components/
│   │   ├── VideoSelector.tsx   # Drag & drop / file picker
│   │   ├── ProcessingView.tsx  # Progress avec étapes
│   │   ├── SettingsPanel.tsx   # Mode selection
│   │   ├── ResultView.tsx      # Stats + open folder
│   │   └── ui/                 # shadcn components
│   ├── hooks/
│   │   └── useProcessor.ts     # Hook pour appeler Tauri
│   ├── lib/
│   │   └── utils.ts
│   └── styles/
│       └── globals.css
├── package.json
├── vite.config.ts
├── tailwind.config.js
└── tsconfig.json
```

## 🎨 DESIGN - OBLIGATOIRE (CRITIQUE)

### Références visuelles
Style inspiré de:
- **Linear** (linear.app) - Clean, minimal, professional
- **Raycast** - Dark mode parfait
- **Arc Browser** - Moderne, élégant

### Couleurs EXACTES
```css
:root {
  --bg-primary: #0A0A0B;      /* Fond principal */
  --bg-secondary: #141415;     /* Cards, panels */
  --bg-tertiary: #1C1C1E;      /* Hover states */
  --border: #2A2A2D;           /* Bordures subtiles */
  --text-primary: #FAFAFA;     /* Texte principal */
  --text-secondary: #A1A1A6;   /* Texte secondaire */
  --accent: #6366F1;           /* Indigo - accent principal */
  --accent-hover: #818CF8;     /* Accent hover */
  --success: #22C55E;          /* Vert succès */
  --error: #EF4444;            /* Rouge erreur */
}
```

### Typography
- **Font**: Inter (Google Fonts) ou SF Pro (system)
- **Headings**: font-semibold, tracking-tight
- **Body**: font-normal, text-sm ou text-base

### Layout
```
┌─────────────────────────────────────────────────────────┐
│  ╔═══════════════════════════════════════════════════╗  │
│  ║                    AutoTrim                       ║  │
│  ║                                                   ║  │
│  ║   ┌───────────────────────────────────────────┐   ║  │
│  ║   │                                           │   ║  │
│  ║   │         Drop your video here              │   ║  │
│  ║   │              or click to                  │   ║  │
│  ║   │            browse files                   │   ║  │
│  ║   │                                           │   ║  │
│  ║   │            📁  Select Video               │   ║  │
│  ║   │                                           │   ║  │
│  ║   └───────────────────────────────────────────┘   ║  │
│  ║                                                   ║  │
│  ║   Processing Mode                                 ║  │
│  ║   ┌─────────┐ ┌─────────┐ ┌─────────────┐        ║  │
│  ║   │Aggressive│ │Moderate │ │Conservative │        ║  │
│  ║   └─────────┘ └─────────┘ └─────────────┘        ║  │
│  ║                                                   ║  │
│  ║   ☑ Remove silences                              ║  │
│  ║   ☑ Remove repetitions (keep last)              ║  │
│  ║                                                   ║  │
│  ╚═══════════════════════════════════════════════════╝  │
└─────────────────────────────────────────────────────────┘
```

### Components Style

**Buttons:**
```tsx
// Primary button
<button className="bg-indigo-600 hover:bg-indigo-500 text-white px-4 py-2 rounded-lg font-medium transition-colors">
  Start Processing
</button>

// Secondary button
<button className="bg-white/5 hover:bg-white/10 text-white px-4 py-2 rounded-lg font-medium border border-white/10 transition-colors">
  Cancel
</button>
```

**Cards:**
```tsx
<div className="bg-[#141415] border border-[#2A2A2D] rounded-xl p-6">
  {/* Content */}
</div>
```

**Drop zone:**
```tsx
<div className="border-2 border-dashed border-[#2A2A2D] hover:border-indigo-500/50 rounded-xl p-12 transition-colors cursor-pointer bg-[#141415]/50">
  {/* Content */}
</div>
```

**Progress bar:**
```tsx
<div className="h-2 bg-[#1C1C1E] rounded-full overflow-hidden">
  <div className="h-full bg-gradient-to-r from-indigo-600 to-indigo-400 transition-all duration-300" style={{ width: `${progress}%` }} />
</div>
```

### Animations (Framer Motion)
- Fade in pour les transitions de vue
- Scale subtil sur les boutons hover
- Progress bar animée smooth
- Skeleton loading pendant le processing

## 📋 Fonctionnalités

### 1. Sélection Vidéo
- Drag & drop sur la fenêtre
- Click pour ouvrir file picker
- Formats supportés: MP4, MOV, MKV, AVI, WEBM
- **AUCUNE limite de taille**
- Afficher: nom, taille, durée estimée

### 2. Settings
- **Mode**: Aggressive / Moderate / Conservative
- **Options**:
  - ☑ Remove silences
  - ☑ Remove repetitions
- **Advanced** (collapsible):
  - Silence threshold (dB)
  - Min silence duration (s)
  - Repetition similarity (%)

### 3. Processing
- Étapes affichées:
  1. Extracting audio...
  2. Transcribing with Whisper...
  3. Detecting silences...
  4. Detecting repetitions...
  5. Rendering final video...
- Progress bar global
- Temps estimé restant
- Bouton Cancel

### 4. Result
- ✅ Processing complete!
- Stats:
  - Original duration: 2h 15m
  - Final duration: 1h 48m
  - Time saved: 27 minutes (20%)
  - Silences removed: 45
  - Repetitions removed: 12
- Boutons:
  - "Open in Finder/Explorer"
  - "Process another video"

## 🔧 Backend Tauri (Rust)

### Commands à implémenter

```rust
#[tauri::command]
async fn select_video() -> Result<VideoInfo, String>

#[tauri::command]
async fn start_processing(
    path: String,
    settings: ProcessingSettings
) -> Result<String, String>  // Returns job_id

#[tauri::command]
async fn get_progress(job_id: String) -> Result<Progress, String>

#[tauri::command]
async fn cancel_processing(job_id: String) -> Result<(), String>

#[tauri::command]
async fn open_output_folder(path: String) -> Result<(), String>
```

### Structs

```rust
#[derive(Serialize, Deserialize)]
struct VideoInfo {
    path: String,
    name: String,
    size_bytes: u64,
    duration_seconds: f64,
}

#[derive(Serialize, Deserialize)]
struct ProcessingSettings {
    mode: String,  // "aggressive" | "moderate" | "conservative"
    remove_silences: bool,
    remove_repetitions: bool,
    silence_threshold_db: f64,
    min_silence_duration: f64,
    repetition_threshold: f64,
}

#[derive(Serialize, Deserialize)]
struct Progress {
    stage: String,
    progress: f64,  // 0-100
    eta_seconds: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct ProcessingResult {
    output_path: String,
    original_duration: f64,
    final_duration: f64,
    silences_removed: u32,
    repetitions_removed: u32,
}
```

### FFmpeg Integration

```rust
use std::process::Command;

fn detect_silences(audio_path: &str, threshold_db: f64, min_duration: f64) -> Vec<Segment> {
    let output = Command::new("ffmpeg")
        .args([
            "-i", audio_path,
            "-af", &format!("silencedetect=n={}dB:d={}", threshold_db, min_duration),
            "-f", "null",
            "-"
        ])
        .output()
        .expect("FFmpeg failed");
    
    // Parse output for silence_start/silence_end
    parse_silence_output(&String::from_utf8_lossy(&output.stderr))
}

fn render_video(input: &str, segments: &[Segment], output: &str) -> Result<(), String> {
    // Generate filter_complex for segment concatenation
    // Use -c copy when possible for speed
}
```

### Whisper API Integration

```rust
use reqwest;
use serde_json::json;

async fn transcribe(audio_path: &str, api_key: &str) -> Result<Transcription, String> {
    let client = reqwest::Client::new();
    
    // Read audio file
    let audio_data = std::fs::read(audio_path)?;
    
    // Create multipart form
    let form = reqwest::multipart::Form::new()
        .text("model", "whisper-1")
        .text("response_format", "verbose_json")
        .text("timestamp_granularity", "word")
        .part("file", reqwest::multipart::Part::bytes(audio_data)
            .file_name("audio.wav"));
    
    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await?;
    
    // Parse response with word-level timestamps
    Ok(response.json().await?)
}
```

## ⚙️ Configuration

### API Key Storage
- Stocker dans le keychain système (secure)
- Ou fichier config local: `~/.autotrim/config.json`
- Premier lancement: demander la clé via UI

### Settings Defaults
```json
{
  "aggressive": {
    "silence_threshold_db": -25,
    "min_silence_duration": 0.3,
    "repetition_threshold": 0.7
  },
  "moderate": {
    "silence_threshold_db": -30,
    "min_silence_duration": 0.5,
    "repetition_threshold": 0.8
  },
  "conservative": {
    "silence_threshold_db": -35,
    "min_silence_duration": 1.0,
    "repetition_threshold": 0.9
  }
}
```

## 🚀 Build & Distribution

### Development
```bash
# Install dependencies
npm install
cd src-tauri && cargo build

# Run dev
npm run tauri dev
```

### Build for production
```bash
npm run tauri build
# Outputs:
# - macOS: .dmg + .app
# - Windows: .msi + .exe
# - Linux: .deb + .AppImage
```

## ✅ Critères de Succès

1. **Design premium** - Niveau Linear/Raycast (pas de design amateur)
2. **Aucune limite de taille** - Vidéos 4K 2h acceptées
3. **Processing fonctionne** - Silences + répétitions détectés
4. **Output correct** - Vidéo finale jouable et correcte
5. **UX fluide** - Pas de freeze, progress visible
6. **Cross-platform** - Au minimum macOS (Jeremy's machine)

## 📝 Notes

- FFmpeg doit être installé sur la machine OU bundlé avec l'app
- Pour le MVP: assumer FFmpeg installé, ajouter check au démarrage
- OpenAI API key: à configurer au premier lancement
- Output: même dossier que l'input avec suffix "_trimmed"
