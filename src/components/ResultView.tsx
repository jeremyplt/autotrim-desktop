import { motion } from 'framer-motion'
import { CheckCircle2, FolderOpen, RotateCcw, Clock, Scissors } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { Button } from './ui/button'
import { Card, CardContent } from './ui/card'
import { formatDuration } from '../lib/utils'

interface ProcessingResult {
  output_path: string
  original_duration: number
  final_duration: number
  silences_removed: number
  repetitions_removed: number
}

interface ResultViewProps {
  result: ProcessingResult
  onReset: () => void
}

export function ResultView({ result, onReset }: ResultViewProps) {
  const timeSaved = result.original_duration - result.final_duration
  const percentageSaved = ((timeSaved / result.original_duration) * 100).toFixed(1)

  const handleOpenFolder = async () => {
    try {
      await invoke('open_output_folder', { path: result.output_path })
    } catch (error) {
      console.error('Failed to open folder:', error)
    }
  }

  const stats = [
    {
      label: 'Original Duration',
      value: formatDuration(result.original_duration),
      icon: Clock,
      color: 'text-[#A1A1A6]',
    },
    {
      label: 'Final Duration',
      value: formatDuration(result.final_duration),
      icon: Clock,
      color: 'text-accent',
    },
    {
      label: 'Time Saved',
      value: `${formatDuration(timeSaved)} (${percentageSaved}%)`,
      icon: Scissors,
      color: 'text-success',
    },
  ]

  const removals = [
    {
      label: 'Silences Removed',
      value: result.silences_removed,
    },
    {
      label: 'Repetitions Removed',
      value: result.repetitions_removed,
    },
  ]

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      className="space-y-6"
    >
      {/* Success Header */}
      <motion.div
        initial={{ opacity: 0, y: -20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.1 }}
        className="text-center"
      >
        <motion.div
          initial={{ scale: 0 }}
          animate={{ scale: 1 }}
          transition={{ delay: 0.2, type: 'spring', stiffness: 200 }}
          className="inline-flex items-center justify-center w-20 h-20 bg-success/10 rounded-2xl mb-4"
        >
          <CheckCircle2 className="w-10 h-10 text-success" />
        </motion.div>
        <h2 className="text-3xl font-bold mb-2">Processing Complete!</h2>
        <p className="text-[#A1A1A6]">Your video has been successfully trimmed</p>
      </motion.div>

      {/* Stats Grid */}
      <Card>
        <CardContent className="pt-6">
          <div className="space-y-4">
            {stats.map((stat, index) => {
              const Icon = stat.icon
              return (
                <motion.div
                  key={stat.label}
                  initial={{ opacity: 0, x: -20 }}
                  animate={{ opacity: 1, x: 0 }}
                  transition={{ delay: 0.3 + index * 0.1 }}
                  className="flex items-center justify-between p-4 bg-[#1C1C1E] rounded-lg"
                >
                  <div className="flex items-center gap-3">
                    <div className={`p-2 bg-white/5 rounded-lg ${stat.color}`}>
                      <Icon className="w-5 h-5" />
                    </div>
                    <span className="text-sm text-[#A1A1A6]">{stat.label}</span>
                  </div>
                  <span className={`font-semibold text-lg ${stat.color}`}>
                    {stat.value}
                  </span>
                </motion.div>
              )
            })}
          </div>

          <div className="mt-6 pt-6 border-t border-[#2A2A2D]">
            <div className="grid grid-cols-2 gap-4">
              {removals.map((item, index) => (
                <motion.div
                  key={item.label}
                  initial={{ opacity: 0, y: 20 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ delay: 0.6 + index * 0.1 }}
                  className="text-center p-4 bg-[#1C1C1E] rounded-lg"
                >
                  <div className="text-3xl font-bold text-accent mb-1">
                    {item.value}
                  </div>
                  <div className="text-sm text-[#A1A1A6]">{item.label}</div>
                </motion.div>
              ))}
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Actions */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.8 }}
        className="flex gap-3"
      >
        <Button
          onClick={handleOpenFolder}
          className="flex-1"
          size="lg"
        >
          <FolderOpen className="w-5 h-5" />
          Open in Finder
        </Button>
        <Button
          onClick={onReset}
          variant="secondary"
          size="lg"
        >
          <RotateCcw className="w-5 h-5" />
          Process Another
        </Button>
      </motion.div>

      {/* Output Path */}
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 1 }}
        className="text-center"
      >
        <p className="text-xs text-[#A1A1A6] font-mono break-all">
          {result.output_path}
        </p>
      </motion.div>
    </motion.div>
  )
}
