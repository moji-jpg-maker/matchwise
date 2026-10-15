'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { BuiltInAIAPI } from '@/lib/builtin-ai';
import {
  applyDownloadEvent,
  DownloadEvent,
  DownloadState,
  isCancelledError,
  startDownloadState,
} from '@/lib/download-progress';

/**
 * Tracks a built-in model download using the canonical `builtin-ai-download-progress` event.
 * State is cleared only by terminal events (completed, cancelled, error), never by the invoke
 * promise settling. Events for other models are ignored while a download is active.
 * Error toasts are owned by the global DownloadProgressToastProvider, and a user cancel
 * (a `CANCELLED:` rejection) is silent, so the invoke rejection is deliberately not toasted.
 */
export function useBuiltInDownload(
  selectedModel: string,
  onFinished?: (model: string, status: 'completed' | 'error' | 'cancelled') => void,
) {
  const [download, setDownload] = useState<DownloadState | null>(null);
  const downloadRef = useRef<DownloadState | null>(null);
  const selectedRef = useRef(selectedModel);
  const finishedRef = useRef(onFinished);

  useEffect(() => { selectedRef.current = selectedModel; }, [selectedModel]);
  useEffect(() => { finishedRef.current = onFinished; }, [onFinished]);

  const update = useCallback((next: DownloadState | null) => {
    downloadRef.current = next;
    setDownload(next);
  }, []);

  useEffect(() => {
    const un = listen<DownloadEvent>('builtin-ai-download-progress', (ev) => {
      const tracked = downloadRef.current?.model ?? selectedRef.current;
      const r = applyDownloadEvent(downloadRef.current, ev.payload, tracked);
      update(r.state);
      if (r.finished) finishedRef.current?.(tracked, r.finished);
    });
    return () => { void un.then((f) => f()); };
  }, [update]);

  const start = useCallback((target: string) => {
    update(startDownloadState(target));
    BuiltInAIAPI.downloadModel(target).catch((e) => {
      // A cancel or a download failure is always followed by its own terminal event.
      // Only a rejection that never produced an event (for example a refused start) is
      // cleaned up here, and it is not toasted because the global provider owns error toasts.
      if (isCancelledError(e)) return;
      if (downloadRef.current?.model === target && downloadRef.current.progress === 0) update(null);
    });
  }, [update]);

  const cancel = useCallback(() => {
    const target = downloadRef.current?.model;
    if (target) void BuiltInAIAPI.cancelDownload(target).catch(() => undefined);
  }, []);

  return { download, start, cancel };
}
