import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { motion, AnimatePresence } from 'framer-motion'
import { Sparkles, AlertCircle } from 'lucide-react'
import { VideoSelector } from './components/VideoSelector'
import { SettingsPanel, ProcessingSettings } from './components/SettingsPanel'
import { ProcessingView } from './components/ProcessingView'
import { ResultView } from './components/ResultView'
import { Button } from './components/ui/button'
import './styles/globals.css'

interface VideoInfo {
  path: string
  name: string
  size_bytes: number
  duration_seconds: number
}

interface ProcessingResult {
  output_path: string
  original_duration: number
  final_duration: number
  silences_removed: number
  repetitions_removed: number
}

type AppState = 'select' | 'processing' | 'complete' | 'error'

function App() {
  const [state, setState] = useState<AppState>('select')
  const [video, setVideo] = useState<VideoInfo | null>(null)
  const [settings, setSettings] = useState<ProcessingSettings>({
    mode: 'moderate',
    remove_silences: true,
    remove_repetitions: true,
    silence_threshold_db: -30,
    min_silence_duration: 0.5,
    repetition_threshold: 0.8,
  })
  const [jobId, setJobId] = useState<string | null>(null)
  const [result, setResult] = useState<ProcessingResult | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [ffmpegInstalled, setFfmpegInstalled] = useState(true)

  useEffect(() => {
    checkFFmpeg()
  }, [])

  const checkFFmpeg = async () => {
    try {
      const installed = await invoke<boolean>('check_ffmpeg')
      setFfmpegInstalled(installed)
    } catch (error) {
      console.error('Failed to check FFmpeg:', error)
      setFfmpegInstalled(false)
    }
  }

  const handleVideoSelected = (videoInfo: VideoInfo) => {
    setVideo(videoInfo)
    setError(null)
  }

  const handleStartProcessing = async () => {
    if (!video) return

    try {
      const newJobId = await invoke<string>('start_processing', {
        path: video.path,
        settings,
      })
      setJobId(newJobId)
      setState('processing')
    } catch (error) {
      console.error('Failed to start processing:', error)
      setError(error as string)
      setState('error')
    }
  }

  const handleProcessingComplete = (processingResult: ProcessingResult) => {
    setResult(processingResult)
    setState('complete')
  }

  const handleCancel = () => {
    setState('select')
    setJobId(null)
  }

  const handleReset = () => {
    setState('select')
    setVideo(null)
    setJobId(null)
    setResult(null)
    setError(null)
  }

  if (!ffmpegInstalled) {
    return (
      <div className="min-h-screen flex items-center justify-center p-8">
        <div className="max-w-md text-center">
          <div className="w-16 h-16 bg-error/10 rounded-2xl flex items-center justify-center mx-auto mb-6">
            <AlertCircle className="w-8 h-8 text-error" />
          </div>
          <h1 className="text-2xl font-bold mb-4">FFmpeg Not Found</h1>
          <p className="text-[#A1A1A6] mb-6">
            AutoTrim requires FFmpeg to process videos. Please install FFmpeg and restart the application.
          </p>
          <div className="bg-[#141415] border border-[#2A2A2D] rounded-xl p-4 text-left">
            <p className="text-sm font-semibold mb-2">Installation:</p>
            <code className="text-xs text-[#A1A1A6] font-mono block">
              # macOS (Homebrew)<br />
              brew install ffmpeg<br />
              <br />
              # Windows (Chocolatey)<br />
              choco install ffmpeg<br />
              <br />
              # Linux (apt)<br />
              sudo apt install ffmpeg
            </code>
          </div>
          <Button onClick={checkFFmpeg} className="mt-6">
            Check Again
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className="min-h-screen p-8">
      <div className="max-w-4xl mx-auto">
        {/* Header */}
        <motion.header
          initial={{ opacity: 0, y: -20 }}
          animate={{ opacity: 1, y: 0 }}
          className="text-center mb-12"
        >
          <div className="inline-flex items-center gap-2 mb-4">
            <Sparkles className="w-8 h-8 text-accent" />
            <h1 className="text-4xl font-bold tracking-tight">AutoTrim</h1>
          </div>
          <p className="text-[#A1A1A6] text-lg">
            Automatically remove silences and repetitions from your videos
          </p>
        </motion.header>

        {/* Main Content */}
        <AnimatePresence mode="wait">
          {state === 'select' && (
            <motion.div
              key="select"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="space-y-6"
            >
              <VideoSelector onVideoSelected={handleVideoSelected} />
              
              {video && (
                <>
                  <SettingsPanel
                    settings={settings}
                    onSettingsChange={setSettings}
                  />
                  
                  <motion.div
                    initial={{ opacity: 0, y: 20 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ delay: 0.2 }}
                  >
                    <Button
                      onClick={handleStartProcessing}
                      size="lg"
                      className="w-full text-lg py-6"
                      disabled={!settings.remove_silences && !settings.remove_repetitions}
                    >
                      <Sparkles className="w-5 h-5" />
                      Start Processing
                    </Button>
                    {!settings.remove_silences && !settings.remove_repetitions && (
                      <p className="text-sm text-error text-center mt-2">
                        Please select at least one option to process
                      </p>
                    )}
                  </motion.div>
                </>
              )}
            </motion.div>
          )}

          {state === 'processing' && jobId && (
            <motion.div
              key="processing"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            >
              <ProcessingView
                jobId={jobId}
                onComplete={handleProcessingComplete}
                onCancel={handleCancel}
              />
            </motion.div>
          )}

          {state === 'complete' && result && (
            <motion.div
              key="complete"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            >
              <ResultView result={result} onReset={handleReset} />
            </motion.div>
          )}

          {state === 'error' && (
            <motion.div
              key="error"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="text-center"
            >
              <div className="w-16 h-16 bg-error/10 rounded-2xl flex items-center justify-center mx-auto mb-6">
                <AlertCircle className="w-8 h-8 text-error" />
              </div>
              <h2 className="text-2xl font-bold mb-4">Processing Failed</h2>
              <p className="text-[#A1A1A6] mb-6">{error || 'An unknown error occurred'}</p>
              <Button onClick={handleReset}>Try Again</Button>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  )
}

export default App
