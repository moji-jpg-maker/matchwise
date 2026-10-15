import React from 'react';
import { useI18n } from '@/i18n';

export interface ChunkStatus {
  chunk_id: number;
  status: 'pending' | 'processing' | 'completed' | 'failed';
  start_time?: number;
  end_time?: number;
  duration_ms?: number;
  text_preview?: string;
  error_message?: string;
}

export interface ProcessingProgress {
  total_chunks: number;
  completed_chunks: number;
  processing_chunks: number;
  failed_chunks: number;
  estimated_remaining_ms?: number;
  chunks: ChunkStatus[];
}

interface ChunkProgressDisplayProps {
  progress: ProcessingProgress;
  onPause?: () => void;
  onResume?: () => void;
  onCancel?: () => void;
  isPaused?: boolean;
  className?: string;
}

export function ChunkProgressDisplay({
  progress,
  onPause,
  onResume,
  onCancel,
  isPaused = false,
  className = ''
}: ChunkProgressDisplayProps) {
  const { t, fmt } = useI18n();
  const completionPercentage = progress.total_chunks > 0
    ? Math.round((progress.completed_chunks / progress.total_chunks) * 100)
    : 0;

  const formatDuration = (ms: number) => {
    const seconds = Math.floor(ms / 1000);
    const minutes = Math.floor(seconds / 60);
    const hours = Math.floor(minutes / 60);

    if (hours > 0) {
      return t('chunkProgress.durHms', { h: fmt.number(hours), m: fmt.number(minutes % 60), s: fmt.number(seconds % 60) });
    } else if (minutes > 0) {
      return t('chunkProgress.durMs', { m: fmt.number(minutes), s: fmt.number(seconds % 60) });
    } else {
      return t('chunkProgress.durS', { s: fmt.number(seconds) });
    }
  };

  const formatTimeRemaining = (ms?: number) => {
    if (!ms || ms <= 0) return t('chunkProgress.calculating');
    return formatDuration(ms);
  };

  const getChunkStatusIcon = (status: ChunkStatus['status']) => {
    switch (status) {
      case 'completed':
        return '✅';
      case 'processing':
        return '⚡';
      case 'failed':
        return '❌';
      case 'pending':
      default:
        return '⏳';
    }
  };

  const getChunkStatusColor = (status: ChunkStatus['status']) => {
    switch (status) {
      case 'completed':
        return 'text-green-600 bg-green-50 border-green-200';
      case 'processing':
        return 'text-blue-600 bg-blue-50 border-blue-200';
      case 'failed':
        return 'text-red-600 bg-red-50 border-red-200';
      case 'pending':
      default:
        return 'text-gray-600 bg-gray-50 border-gray-200';
    }
  };

  return (
    <div className={`bg-white border border-gray-200 rounded-lg p-4 ${className}`}>
      {/* Progress Header */}
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center space-x-3 rtl:space-x-reverse">
          <h3 className="text-lg font-semibold text-gray-900">
            {t('chunkProgress.title')}
          </h3>
          {isPaused && (
            <span className="bg-yellow-100 text-yellow-800 px-2 py-1 rounded-full text-xs font-medium">
              {t('chunkProgress.paused')}
            </span>
          )}
        </div>

        <div className="flex items-center space-x-2 rtl:space-x-reverse">
          {!isPaused ? (
            <button
              onClick={onPause}
              className="bg-yellow-500 hover:bg-yellow-600 text-white px-3 py-1 rounded text-sm transition-colors"
              disabled={progress.processing_chunks === 0 && progress.completed_chunks === progress.total_chunks}
            >
              {t('chunkProgress.pause')}
            </button>
          ) : (
            <button
              onClick={onResume}
              className="bg-green-500 hover:bg-green-600 text-white px-3 py-1 rounded text-sm transition-colors"
            >
              {t('chunkProgress.resume')}
            </button>
          )}

          <button
            onClick={onCancel}
            className="bg-red-500 hover:bg-red-600 text-white px-3 py-1 rounded text-sm transition-colors"
          >
            {t('chunkProgress.cancel')}
          </button>
        </div>
      </div>

      {/* Progress Bar */}
      <div className="mb-4">
        <div className="flex items-center justify-between mb-2">
          <span className="text-sm font-medium text-gray-700">
            {t('chunkProgress.completedOf', { done: fmt.number(progress.completed_chunks), total: fmt.number(progress.total_chunks) })}
          </span>
          <span className="text-sm font-medium text-gray-700">
            {fmt.percent(completionPercentage / 100)}
          </span>
        </div>

        <div className="w-full bg-gray-200 rounded-full h-2">
          <div
            className="bg-blue-600 h-2 rounded-full transition-all duration-300 ease-out"
            style={{ width: `${completionPercentage}%` }}
          />
        </div>
      </div>

      {/* Processing Stats */}
      <div className="grid grid-cols-4 gap-4 mb-4 text-sm">
        <div className="text-center">
          <div className="text-lg font-semibold text-green-600">
            {fmt.number(progress.completed_chunks)}
          </div>
          <div className="text-gray-600">{t('chunkProgress.completed')}</div>
        </div>

        <div className="text-center">
          <div className="text-lg font-semibold text-blue-600">
            {fmt.number(progress.processing_chunks)}
          </div>
          <div className="text-gray-600">{t('chunkProgress.processing')}</div>
        </div>

        <div className="text-center">
          <div className="text-lg font-semibold text-gray-600">
            {fmt.number(progress.total_chunks - progress.completed_chunks - progress.processing_chunks - progress.failed_chunks)}
          </div>
          <div className="text-gray-600">{t('chunkProgress.pending')}</div>
        </div>

        <div className="text-center">
          <div className="text-lg font-semibold text-red-600">
            {fmt.number(progress.failed_chunks)}
          </div>
          <div className="text-gray-600">{t('chunkProgress.failed')}</div>
        </div>
      </div>

      {/* Time Estimate */}
      {progress.estimated_remaining_ms && progress.estimated_remaining_ms > 0 && (
        <div className="bg-blue-50 border border-blue-200 rounded-lg p-3 mb-4">
          <div className="flex items-center space-x-2 rtl:space-x-reverse">
            <span className="text-blue-600">⏱️</span>
            <span className="text-sm text-blue-800">
              {t('chunkProgress.eta', { value: formatTimeRemaining(progress.estimated_remaining_ms) })}
            </span>
          </div>
        </div>
      )}

      {/* Recent Chunks Grid */}
      <div className="space-y-2">
        <h4 className="text-sm font-medium text-gray-700 mb-2">
          {t('chunkProgress.recent', { shown: fmt.number(Math.min(progress.chunks.length, 10)), total: fmt.number(progress.total_chunks) })}
        </h4>

        <div className="max-h-48 overflow-y-auto space-y-1">
          {progress.chunks
            .slice(-10) // Show last 10 chunks
            .reverse() // Most recent first
            .map((chunk) => (
              <div
                key={chunk.chunk_id}
                className={`text-xs p-2 rounded border ${getChunkStatusColor(chunk.status)}`}
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center space-x-2 rtl:space-x-reverse">
                    <span>{getChunkStatusIcon(chunk.status)}</span>
                    <span className="font-medium">
                      {t('chunkProgress.chunk', { id: fmt.number(chunk.chunk_id) })}
                    </span>
                    {chunk.duration_ms && (
                      <span className="text-gray-500">
                        ({formatDuration(chunk.duration_ms)})
                      </span>
                    )}
                  </div>

                  {chunk.status === 'processing' && (
                    <div className="flex items-center space-x-1 rtl:space-x-reverse">
                      <div className="animate-spin w-3 h-3 border border-blue-600 border-t-transparent rounded-full"></div>
                    </div>
                  )}
                </div>

                {chunk.text_preview && (
                  <div dir="auto" className="mt-1 text-gray-700 text-xs truncate">
                    "{chunk.text_preview}"
                  </div>
                )}

                {chunk.error_message && (
                  <div className="mt-1 text-red-700 text-xs">
                    {t('chunkProgress.error', { message: chunk.error_message })}
                  </div>
                )}
              </div>
            ))}
        </div>
      </div>

      {/* Processing Complete */}
      {progress.completed_chunks === progress.total_chunks && progress.total_chunks > 0 && (
        <div className="mt-4 bg-green-50 border border-green-200 rounded-lg p-3">
          <div className="flex items-center space-x-2 rtl:space-x-reverse">
            <span className="text-green-600">🎉</span>
            <span className="text-sm font-medium text-green-800">
              {t('chunkProgress.done', { total: fmt.number(progress.total_chunks) })}
            </span>
          </div>
        </div>
      )}
    </div>
  );
}

// Mini version for sidebar or compact display
export function ChunkProgressMini({ progress, className = '' }: { progress: ProcessingProgress; className?: string }) {
  const { t, fmt } = useI18n();
  const completionPercentage = progress.total_chunks > 0
    ? Math.round((progress.completed_chunks / progress.total_chunks) * 100)
    : 0;

  return (
    <div className={`bg-gray-50 border border-gray-200 rounded-lg p-3 ${className}`}>
      <div className="flex items-center justify-between mb-2">
        <span className="text-sm font-medium text-gray-700">
          {t('chunkProgress.processing')}
        </span>
        <span className="text-sm font-medium text-gray-700">
          {fmt.percent(completionPercentage / 100)}
        </span>
      </div>

      <div className="w-full bg-gray-200 rounded-full h-1.5 mb-2">
        <div
          className="bg-blue-600 h-1.5 rounded-full transition-all duration-300"
          style={{ width: `${completionPercentage}%` }}
        />
      </div>

      <div className="text-xs text-gray-600">
        {t('chunkProgress.miniChunks', { done: fmt.number(progress.completed_chunks), total: fmt.number(progress.total_chunks) })}
        {progress.processing_chunks > 0 && (
          <span className="ms-2 text-blue-600">
            {t('chunkProgress.miniProcessing', { count: fmt.number(progress.processing_chunks) })}
          </span>
        )}
      </div>
    </div>
  );
}