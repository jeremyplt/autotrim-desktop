import { useState } from 'react'
import { motion } from 'framer-motion'
import { Check } from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from './ui/card'

export interface ProcessingSettings {
  mode: 'aggressive' | 'moderate' | 'conservative'
  remove_silences: boolean
  remove_repetitions: boolean
  silence_threshold_db: number
  min_silence_duration: number
  repetition_threshold: number
}

interface SettingsPanelProps {
  settings: ProcessingSettings
  onSettingsChange: (settings: ProcessingSettings) => void
  disabled?: boolean
}

const modePresets = {
  aggressive: {
    silence_threshold_db: -30,
    min_silence_duration: 0.4,
    repetition_threshold: 0.75,
  },
  moderate: {
    silence_threshold_db: -35,
    min_silence_duration: 0.75,
    repetition_threshold: 0.85,
  },
  conservative: {
    silence_threshold_db: -40,
    min_silence_duration: 1.5,
    repetition_threshold: 0.92,
  },
}

const modes = [
  {
    id: 'aggressive' as const,
    name: 'Aggressive',
    description: 'Maximum trimming, fastest results',
  },
  {
    id: 'moderate' as const,
    name: 'Moderate',
    description: 'Balanced approach (recommended)',
  },
  {
    id: 'conservative' as const,
    name: 'Conservative',
    description: 'Minimal trimming, safest option',
  },
]

export function SettingsPanel({ settings, onSettingsChange, disabled }: SettingsPanelProps) {
  const handleModeChange = (mode: 'aggressive' | 'moderate' | 'conservative') => {
    onSettingsChange({
      ...settings,
      mode,
      ...modePresets[mode],
    })
  }

  const handleToggle = (key: 'remove_silences' | 'remove_repetitions') => {
    onSettingsChange({
      ...settings,
      [key]: !settings[key],
    })
  }

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.1 }}
      className="space-y-6"
    >
      <Card>
        <CardHeader>
          <CardTitle>Processing Mode</CardTitle>
          <CardDescription>Choose how aggressively to trim your video</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-3 gap-3">
            {modes.map((mode) => (
              <button
                key={mode.id}
                onClick={() => handleModeChange(mode.id)}
                disabled={disabled}
                className={`
                  relative p-4 rounded-lg border-2 transition-all text-left
                  disabled:opacity-50 disabled:cursor-not-allowed
                  ${settings.mode === mode.id
                    ? 'border-accent bg-accent/5'
                    : 'border-[#2A2A2D] bg-[#1C1C1E] hover:border-accent/50'
                  }
                `}
              >
                {settings.mode === mode.id && (
                  <div className="absolute top-3 right-3 w-5 h-5 bg-accent rounded-full flex items-center justify-center">
                    <Check className="w-3 h-3 text-white" />
                  </div>
                )}
                <div className="font-semibold mb-1">{mode.name}</div>
                <div className="text-xs text-[#A1A1A6]">{mode.description}</div>
              </button>
            ))}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Options</CardTitle>
          <CardDescription>Select what to remove from your video</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <label className="flex items-start gap-3 cursor-pointer group">
            <div className="relative flex items-center justify-center mt-0.5">
              <input
                type="checkbox"
                checked={settings.remove_silences}
                onChange={() => handleToggle('remove_silences')}
                disabled={disabled}
                className="sr-only peer"
              />
              <div className="w-5 h-5 border-2 border-[#2A2A2D] rounded peer-checked:border-accent peer-checked:bg-accent transition-all group-hover:border-accent/50 flex items-center justify-center">
                {settings.remove_silences && <Check className="w-3 h-3 text-white" />}
              </div>
            </div>
            <div className="flex-1">
              <div className="font-medium">Remove silences</div>
              <div className="text-sm text-[#A1A1A6]">
                Automatically detect and remove silent segments
              </div>
            </div>
          </label>

          <label className="flex items-start gap-3 cursor-pointer group">
            <div className="relative flex items-center justify-center mt-0.5">
              <input
                type="checkbox"
                checked={settings.remove_repetitions}
                onChange={() => handleToggle('remove_repetitions')}
                disabled={disabled}
                className="sr-only peer"
              />
              <div className="w-5 h-5 border-2 border-[#2A2A2D] rounded peer-checked:border-accent peer-checked:bg-accent transition-all group-hover:border-accent/50 flex items-center justify-center">
                {settings.remove_repetitions && <Check className="w-3 h-3 text-white" />}
              </div>
            </div>
            <div className="flex-1">
              <div className="font-medium">Remove repetitions</div>
              <div className="text-sm text-[#A1A1A6]">
                Detect and remove repeated phrases (keeps last occurrence)
              </div>
            </div>
          </label>
        </CardContent>
      </Card>
    </motion.div>
  )
}
