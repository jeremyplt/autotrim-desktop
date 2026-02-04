import { useState, useCallback } from 'react'
import { motion } from 'framer-motion'
import { Upload, File, X } from 'lucide-react'
import { open } from '@tauri-apps/api/dialog'
import { invoke } from '@tauri-apps/api/tauri'
import { Button } from './ui/button'
import { formatBytes, formatDuration } from '../lib/utils'

interface VideoInfo {
  path: string
  name: string
  size_bytes: number
  duration_seconds: number
}

interface VideoSelectorProps {
  onVideoSelected: (video: VideoInfo) => void
}

export function VideoSelector({ onVideoSelected }: VideoSelectorProps) {
  const [video, setVideo] = useState<VideoInfo | null>(null)
  const [isDragging, setIsDragging] = useState(false)
  const [isLoading, setIsLoading] = useState(false)

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(true)
  }, [])

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)
  }, [])

  const handleDrop = useCallback(async (e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)
    
    if (e.dataTransfer.files && e.dataTransfer.files[0]) {
      const file = e.dataTransfer.files[0]
      await loadVideoInfo(file.path)
    }
  }, [])

  const handleFileSelect = async () => {
    const selected = await open({
      multiple: false,
      filters: [{
        name: 'Video',
        extensions: ['mp4', 'mov', 'mkv', 'avi', 'webm']
      }]
    })

    if (selected && typeof selected === 'string') {
      await loadVideoInfo(selected)
    }
  }

  const loadVideoInfo = async (path: string) => {
    setIsLoading(true)
    try {
      const videoInfo = await invoke<VideoInfo>('get_video_info', { path })
      setVideo(videoInfo)
      onVideoSelected(videoInfo)
    } catch (error) {
      console.error('Failed to load video info:', error)
    } finally {
      setIsLoading(false)
    }
  }

  const clearVideo = () => {
    setVideo(null)
  }

  if (video) {
    return (
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="bg-[#141415] border border-[#2A2A2D] rounded-xl p-6"
      >
        <div className="flex items-start justify-between">
          <div className="flex items-start gap-4 flex-1">
            <div className="w-12 h-12 bg-accent/10 rounded-lg flex items-center justify-center flex-shrink-0">
              <File className="w-6 h-6 text-accent" />
            </div>
            <div className="flex-1 min-w-0">
              <h3 className="font-semibold text-lg truncate">{video.name}</h3>
              <div className="mt-2 space-y-1">
                <p className="text-sm text-[#A1A1A6]">
                  Size: <span className="text-[#FAFAFA]">{formatBytes(video.size_bytes)}</span>
                </p>
                <p className="text-sm text-[#A1A1A6]">
                  Duration: <span className="text-[#FAFAFA]">{formatDuration(video.duration_seconds)}</span>
                </p>
                <p className="text-sm text-[#A1A1A6] truncate">
                  Path: <span className="text-[#FAFAFA] font-mono text-xs">{video.path}</span>
                </p>
              </div>
            </div>
          </div>
          <Button
            variant="ghost"
            size="icon"
            onClick={clearVideo}
            className="flex-shrink-0"
          >
            <X className="w-4 h-4" />
          </Button>
        </div>
      </motion.div>
    )
  }

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      className={`
        border-2 border-dashed rounded-xl p-12 transition-all cursor-pointer
        bg-[#141415]/50 backdrop-blur-sm
        ${isDragging 
          ? 'border-accent/50 bg-accent/5' 
          : 'border-[#2A2A2D] hover:border-accent/30 hover:bg-[#141415]'
        }
      `}
      onClick={handleFileSelect}
    >
      <div className="flex flex-col items-center justify-center text-center">
        <motion.div
          animate={isDragging ? { scale: 1.1 } : { scale: 1 }}
          className="w-16 h-16 bg-accent/10 rounded-xl flex items-center justify-center mb-4"
        >
          <Upload className="w-8 h-8 text-accent" />
        </motion.div>
        <h3 className="text-xl font-semibold mb-2">
          {isDragging ? 'Drop your video here' : 'Select a video file'}
        </h3>
        <p className="text-[#A1A1A6] mb-6">
          Drag and drop or click to browse
        </p>
        <div className="flex flex-wrap gap-2 justify-center text-xs text-[#A1A1A6]">
          <span className="px-2 py-1 bg-white/5 rounded">MP4</span>
          <span className="px-2 py-1 bg-white/5 rounded">MOV</span>
          <span className="px-2 py-1 bg-white/5 rounded">MKV</span>
          <span className="px-2 py-1 bg-white/5 rounded">AVI</span>
          <span className="px-2 py-1 bg-white/5 rounded">WEBM</span>
        </div>
        {isLoading && (
          <div className="mt-4">
            <div className="animate-spin w-6 h-6 border-2 border-accent border-t-transparent rounded-full" />
          </div>
        )}
      </div>
    </motion.div>
  )
}
