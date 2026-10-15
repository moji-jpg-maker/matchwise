export type DownloadStatus = 'downloading' | 'completed' | 'error' | 'cancelled';

export interface DownloadEvent {
  model: string;
  progress?: number;
  downloaded_mb?: number;
  total_mb?: number;
  speed_mbps?: number;
  status: DownloadStatus | string;
  error?: string;
}

export interface DownloadState {
  model: string;
  progress: number;
  downloadedMb: number;
  totalMb: number;
  speedMbps: number;
}

export interface DownloadUpdate {
  state: DownloadState | null;
  finished: 'completed' | 'error' | 'cancelled' | null;
  error?: string;
}

export function clampPercent(value: number | undefined | null): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 0;
  return Math.min(100, Math.max(0, value));
}

export function startDownloadState(model: string): DownloadState {
  return { model, progress: 0, downloadedMb: 0, totalMb: 0, speedMbps: 0 };
}

export function applyDownloadEvent(
  current: DownloadState | null,
  event: DownloadEvent,
  trackedModel: string,
): DownloadUpdate {
  if (event.model !== trackedModel) return { state: current, finished: null };
  if (event.status === 'completed') return { state: null, finished: 'completed' };
  if (event.status === 'cancelled') return { state: null, finished: 'cancelled' };
  if (event.status === 'error') return { state: null, finished: 'error', error: event.error };
  if (event.status === 'downloading') {
    return {
      state: {
        model: event.model,
        progress: clampPercent(event.progress),
        downloadedMb: event.downloaded_mb ?? 0,
        totalMb: event.total_mb ?? 0,
        speedMbps: event.speed_mbps ?? 0,
      },
      finished: null,
    };
  }
  return { state: current, finished: null };
}

export function formatDownloadDetail(state: DownloadState): string {
  const parts = [`${Math.round(state.progress)}%`];
  if (state.totalMb > 0) parts.push(`${Math.round(state.downloadedMb)} / ${Math.round(state.totalMb)} MB`);
  if (state.speedMbps > 0) parts.push(`${state.speedMbps.toFixed(1)} MB/s`);
  return parts.join(' · ');
}
