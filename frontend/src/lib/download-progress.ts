import type { Translate } from '@/lib/rules';
import type { Formatters } from '@/i18n/format';

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

/** The backend rejects a user-cancelled download with a `CANCELLED:` prefixed message. */
export function isCancelledError(e: unknown): boolean {
  const msg = e instanceof Error ? e.message : String(e ?? '');
  return msg.startsWith('CANCELLED:');
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

export function formatDownloadDetail(state: DownloadState, t: Translate, fmt: Formatters): string {
  const parts = [fmt.percent(state.progress / 100)];
  if (state.totalMb > 0) {
    parts.push(t('dlDetail.amount', { done: fmt.number(Math.round(state.downloadedMb)), total: fmt.number(Math.round(state.totalMb)) }));
  }
  if (state.speedMbps > 0) {
    parts.push(t('dlDetail.speed', { value: fmt.number(state.speedMbps, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) }));
  }
  return parts.join(' · ');
}
