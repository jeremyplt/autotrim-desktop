import { useEffect, useState } from 'react'
import { motion } from 'framer-motion'
import { Loader2, CheckCircle2 } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { Progress } from './ui/progress'
import { Button } from './ui/button'
import { Card, CardContent } from './ui/card'
import { formatETA } from '../lib/utils'

interface ProcessingProgress {
  stage: string
  progress: number
  eta_seconds?: number
}

interface ProcessingViewProps {
  jobId: string
  onComplete: (result: ProcessingResult) => void
  onCancel: () => void
}

interface ProcessingResult {
  output_path: string
  original_duration: number
  final_duration: number
  silences_removed: number
  repetitions_removed: number
}

const stages = [
  { id: 'extracting', label: 'Extracting audio' },
  { id: 'transcribing', label: 'Transcribing with Whisper' },
  { id: 'detecting_silences', label: 'Detecting silences & dead zones' },
  { id: 'analyzing_retakes', label: 'Analyzing retakes (AI)' },
  { id: 'verifying_cuts', label: 'Verifying cuts (AI)' },
  { id: 'rendering', label: 'Rendering final video' },
]

export function ProcessingView({ jobId, onComplete, onCancel }: ProcessingViewProps) {
  const [progress, setProgress] = useState<ProcessingProgress>({
    stage: 'extracting',
    progress: 0,
  })
  const [isCanceling, setIsCanceling] = useState(false)

  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const currentProgress = await invoke<ProcessingProgress>('get_progress', { jobId })
        setProgress(currentProgress)

        if (currentProgress.progress >= 100) {
          clearInterval(interval)
          const result = await invoke<ProcessingResult>('get_result', { jobId })
          onComplete(result)
        }
      } catch (error) {
        console.error('Failed to get progress:', error)
      }
    }, 500)

    return () => clearInterval(interval)
  }, [jobId, onComplete])

  const handleCancel = async () => {
    setIsCanceling(true)
    try {
      await invoke('cancel_processing', { jobId })
      onCancel()
    } catch (error) {
      console.error('Failed to cancel:', error)
    }
  }

  const currentStageIndex = stages.findIndex(s => s.id === progress.stage)

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      className="space-y-6"
    >
      <Card>
        <CardContent className="pt-6">
          <div className="space-y-6">
            {/* Overall Progress */}
            <div>
              <div className="flex items-center justify-between mb-2">
                <h3 className="text-lg font-semibold">Processing your video</h3>
                <span className="text-sm text-[#A1A1A6]">
                  {Math.round(progress.progress)}%
                </span>
              </div>
              <Progress value={progress.progress} className="h-3" />
              {progress.eta_seconds && progress.eta_seconds > 0 && (
                <p className="text-sm text-[#A1A1A6] mt-2">
                  Estimated time remaining: {formatETA(progress.eta_seconds)}
                </p>
              )}
            </div>

            {/* Stage Progress */}
            <div className="space-y-3">
              {stages.map((stage, index) => {
                const isComplete = index < currentStageIndex
                const isCurrent = index === currentStageIndex
                const isPending = index > currentStageIndex

                return (
                  <motion.div
                    key={stage.id}
                    initial={{ opacity: 0, x: -20 }}
                    animate={{ opacity: 1, x: 0 }}
                    transition={{ delay: index * 0.1 }}
                    className="flex items-center gap-3"
                  >
                    <div className="flex-shrink-0">
                      {isComplete ? (
                        <CheckCircle2 className="w-5 h-5 text-success" />
                      ) : isCurrent ? (
                        <Loader2 className="w-5 h-5 text-accent animate-spin" />
                      ) : (
                        <div className="w-5 h-5 border-2 border-[#2A2A2D] rounded-full" />
                      )}
                    </div>
                    <div className="flex-1">
                      <p className={`text-sm font-medium ${
                        isComplete ? 'text-success' : 
                        isCurrent ? 'text-[#FAFAFA]' : 
                        'text-[#A1A1A6]'
                      }`}>
                        {stage.label}
                        {isCurrent && '...'}
                      </p>
                    </div>
                  </motion.div>
                )
              })}
            </div>

            {/* Actions */}
            <div className="pt-4 border-t border-[#2A2A2D]">
              <Button
                variant="destructive"
                onClick={handleCancel}
                disabled={isCanceling}
                className="w-full"
              >
                {isCanceling ? (
                  <>
                    <Loader2 className="w-4 h-4 animate-spin" />
                    Canceling...
                  </>
                ) : (
                  'Cancel Processing'
                )}
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>
    </motion.div>
  )
}
