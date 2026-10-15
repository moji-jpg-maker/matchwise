import { describe, expect, test } from 'bun:test';

import {
  applyDownloadEvent,
  clampPercent,
  formatDownloadDetail,
  isCancelledError,
  startDownloadState,
} from '../../src/lib/download-progress';

describe('download progress events', () => {
  const start = startDownloadState('m1');

  test('downloading updates the state', () => {
    const r = applyDownloadEvent(start, { model: 'm1', progress: 42, downloaded_mb: 10, total_mb: 20, speed_mbps: 2, status: 'downloading' }, 'm1');
    expect(r.finished).toBeNull();
    expect(r.state).toEqual({ model: 'm1', progress: 42, downloadedMb: 10, totalMb: 20, speedMbps: 2 });
  });

  test('events for another model are ignored', () => {
    const r = applyDownloadEvent(start, { model: 'other', progress: 90, status: 'downloading' }, 'm1');
    expect(r.state).toBe(start);
    expect(r.finished).toBeNull();
    expect(applyDownloadEvent(null, { model: 'other', status: 'completed' }, 'm1').finished).toBeNull();
  });

  test('progress 100 while downloading is not completion', () => {
    const r = applyDownloadEvent(start, { model: 'm1', progress: 100, status: 'downloading' }, 'm1');
    expect(r.finished).toBeNull();
    expect(r.state?.progress).toBe(100);
  });

  test('completed, cancelled and error clear the state', () => {
    expect(applyDownloadEvent(start, { model: 'm1', progress: 100, status: 'completed' }, 'm1')).toEqual({ state: null, finished: 'completed' });
    expect(applyDownloadEvent(start, { model: 'm1', status: 'cancelled' }, 'm1')).toEqual({ state: null, finished: 'cancelled' });
    expect(applyDownloadEvent(start, { model: 'm1', status: 'error', error: 'boom' }, 'm1')).toEqual({ state: null, finished: 'error', error: 'boom' });
  });

  test('out-of-range and invalid values are clamped', () => {
    expect(clampPercent(150)).toBe(100);
    expect(clampPercent(-5)).toBe(0);
    expect(clampPercent(NaN)).toBe(0);
    expect(clampPercent(undefined)).toBe(0);
    expect(applyDownloadEvent(start, { model: 'm1', progress: 250, status: 'downloading' }, 'm1').state?.progress).toBe(100);
  });

  test('detail text omits unknown parts', () => {
    expect(formatDownloadDetail(start)).toBe('0%');
    expect(formatDownloadDetail({ model: 'm1', progress: 42.4, downloadedMb: 310.2, totalMb: 1200, speedMbps: 5.25 })).toBe('42% · 310 / 1200 MiB · 5.3 MiB/s');
  });

  test('a CANCELLED: rejection is recognised, other errors are not', () => {
    expect(isCancelledError('CANCELLED: Download cancelled by user')).toBe(true);
    expect(isCancelledError(new Error('CANCELLED: x'))).toBe(true);
    expect(isCancelledError('network down')).toBe(false);
    expect(isCancelledError(undefined)).toBe(false);
  });

  test('a foreign error event does not clear the tracked download', () => {
    const r = applyDownloadEvent(start, { model: 'other', status: 'error', error: 'x' }, 'm1');
    expect(r.state).toBe(start);
    expect(r.finished).toBeNull();
  });
});
