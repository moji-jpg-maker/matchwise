import React from 'react';
import { ModelStatus } from '../lib/whisper';
import { useI18n } from '@/i18n';

interface ModelDownloadProgressProps {
  status: ModelStatus;
  modelName: string;
  onCancel?: () => void;
}

export function ModelDownloadProgress({ status, modelName, onCancel }: ModelDownloadProgressProps) {
  const { t, fmt } = useI18n();
  if (typeof status !== 'object' || !('Downloading' in status)) {
    return null;
  }

  const progress = status.Downloading.progress;
  const isCompleted = progress >= 100;

  return (
    <div className="bg-blue-50 border border-blue-200 rounded-lg p-4">
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center space-x-2 rtl:space-x-reverse">
          <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-blue-600"></div>
          <span className="text-sm font-medium text-blue-900">
            {isCompleted ? t('dlProgress.finalizing') : t('dlProgress.downloading', { model: modelName })}
          </span>
        </div>
      </div>
      
      <div className="relative">
        <div className="w-full bg-blue-200 rounded-full h-2" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(Math.min(progress, 100))}>
          <div 
            className="bg-blue-600 h-2 rounded-full transition-all duration-300 ease-out"
            style={{ width: `${Math.min(progress, 100)}%` }}
          />
        </div>
        <div className="flex justify-between text-xs text-blue-700 mt-1">
          <span>{t('dlProgress.complete', { percent: fmt.percent(progress / 100) })}</span>
          {!isCompleted && (
            <span className="animate-pulse">{t('dlProgress.downloadingShort')}</span>
          )}
        </div>
      </div>
      
      {isCompleted && (
        <div className="mt-2 text-xs text-green-700">
          {t('dlProgress.loading')}
        </div>
      )}
    </div>
  );
}

interface ProgressRingProps {
  progress: number;
  size?: number;
  strokeWidth?: number;
}

export function ProgressRing({ progress, size = 40, strokeWidth = 3 }: ProgressRingProps) {
  const { fmt } = useI18n();
  const radius = (size - strokeWidth) / 2;
  const circumference = radius * 2 * Math.PI;
  const strokeDasharray = circumference;
  const strokeDashoffset = circumference - (progress / 100) * circumference;

  return (
    <div className="relative inline-flex items-center justify-center">
      <svg
        width={size}
        height={size}
        className="transform -rotate-90"
      >
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          stroke="#e5e7eb"
          strokeWidth={strokeWidth}
          fill="transparent"
        />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          stroke="#3b82f6"
          strokeWidth={strokeWidth}
          strokeDasharray={strokeDasharray}
          strokeDashoffset={strokeDashoffset}
          strokeLinecap="round"
          fill="transparent"
          className="transition-all duration-300 ease-in-out"
        />
      </svg>
      <span className="absolute text-xs font-medium text-blue-600">
        {fmt.percent(progress / 100)}
      </span>
    </div>
  );
}

interface DownloadSummaryProps {
  totalModels: number;
  downloadedModels: number;
  totalSizeMb: number;
}

export function DownloadSummary({ totalModels, downloadedModels, totalSizeMb }: DownloadSummaryProps) {
  const { t, fmt } = useI18n();
  const formatSize = (mb: number) => {
    if (mb >= 1000) return t('dlProgress.sizeGb', { value: fmt.number(mb / 1000, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) });
    return t('dlProgress.sizeMb', { value: fmt.number(mb) });
  };

  return (
    <div className="bg-gray-50 rounded-lg p-3 text-sm">
      <div className="flex items-center justify-between">
        <span className="text-gray-700">
          {t('dlProgress.available', { done: fmt.number(downloadedModels), total: fmt.number(totalModels) })}
        </span>
        <span className="text-gray-600">
          {t('dlProgress.total', { size: formatSize(totalSizeMb) })}
        </span>
      </div>
      {downloadedModels > 0 && (
        <div className="mt-1 text-xs text-green-600">
          {t('dlProgress.local')}
        </div>
      )}
    </div>
  );
}
