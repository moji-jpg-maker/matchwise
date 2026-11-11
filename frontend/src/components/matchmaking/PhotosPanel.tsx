'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { ImagePlus, Loader2, Send, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { AddPhotosResult, ProfilePhoto } from '@/types/matchmaking';
import { useI18n } from '@/i18n';
import { describeRejections } from '@/lib/photo-rejections';

/** The backend enforces this; the count line repeats it for display. */
const MAX_PHOTOS = 10;

function Thumbnail({
  photo,
  onDelete,
}: {
  photo: ProfilePhoto;
  onDelete: () => void;
}) {
  const { t } = useI18n();
  const [broken, setBroken] = useState(false);
  const usable = photo.path && photo.status === 'ready' && !broken;
  const status = photo.status === 'pending' ? t('photosPanel.statusPending') : photo.status === 'failed' ? t('photosPanel.statusFailed') : null;
  return (
    <div className="relative aspect-square rounded-lg overflow-hidden bg-gray-100 border group">
      {usable ? (
        <img
          src={convertFileSrc(photo.path!)}
          alt={photo.original_name ?? ''}
          loading="lazy"
          onError={() => setBroken(true)}
          className="w-full h-full object-cover"
        />
      ) : (
        <div className="w-full h-full flex items-center justify-center text-gray-400">
          <ImagePlus className="w-6 h-6" aria-hidden />
        </div>
      )}

      <div className="absolute inset-x-0 bottom-0 flex items-center gap-1 px-1.5 py-1 text-[10px] text-white bg-black/55">
        {photo.source === 'telegram' && (
          <>
            <Send className="w-3 h-3 shrink-0" aria-hidden />
            <span className="truncate">{t('photosPanel.fromTelegram')}</span>
          </>
        )}
        {status && <span className="shrink-0 ms-auto rounded bg-black/40 px-1">{status}</span>}
      </div>

      <button
        type="button"
        onClick={onDelete}
        aria-label={t('photosPanel.delete')}
        className="absolute top-1 end-1 rounded bg-white/90 p-1 text-gray-700 opacity-0 group-hover:opacity-100 focus:opacity-100 hover:text-red-700"
      >
        <Trash2 className="w-3.5 h-3.5" aria-hidden />
      </button>
    </div>
  );
}

/**
 * A person's photos: thumbnails, adding several at once, and deleting one. Photos are shown only here and are
 * stored exactly as received, so the source (upload or Telegram) is labelled rather than silently mixed.
 */
export function PhotosPanel({ profileId, onCountChange }: { profileId: string; onCountChange?: (count: number) => void }) {
  const { t, fmt, locale } = useI18n();
  const [photos, setPhotos] = useState<ProfilePhoto[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmId, setConfirmId] = useState<string | null>(null);

  const apply = useCallback(
    (next: ProfilePhoto[]) => {
      setPhotos(next);
      onCountChange?.(next.length);
    },
    [onCountChange],
  );

  const load = useCallback(async () => {
    setLoadError(null);
    try {
      apply(await invoke<ProfilePhoto[]>('mm_profile_photos', { profileId, locale }));
    } catch (e) {
      setLoadError(`${e}`);
    }
  }, [profileId, locale, apply]);

  useEffect(() => { void load(); }, [load]);

  const add = async () => {
    setBusy(true);
    try {
      let paths: string[];
      try {
        paths = await invoke<string[]>('mm_pick_photo_files', { locale });
      } catch (e) {
        toast.error(t('photosPanel.pickFailed', { error: `${e}` }));
        return;
      }
      if (paths.length === 0) return;
      try {
        const res = await invoke<AddPhotosResult>('mm_add_profile_photos', { profileId, paths, locale });
        if (res.photos.length > 0) toast.success(t('photosPanel.added', { count: fmt.number(res.photos.length) }));
        if (res.rejected.length > 0) {
          toast.warning(t('photosPanel.rejected', { items: describeRejections(res.rejected, t) }));
        }
        await load();
      } catch (e) {
        toast.error(t('photosPanel.addFailed', { error: `${e}` }));
      }
    } finally {
      setBusy(false);
    }
  };

  const remove = async (photo: ProfilePhoto) => {
    if (confirmId !== photo.id) {
      setConfirmId(photo.id);
      return;
    }
    setConfirmId(null);
    setBusy(true);
    try {
      await invoke('mm_delete_profile_photo', { profileId, photoId: photo.id, locale });
      toast.success(t('photosPanel.deleted'));
      await load();
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setBusy(false);
    }
  };

  const count = photos?.length ?? 0;
  const full = count >= MAX_PHOTOS;

  return (
    <div className="bg-white rounded-lg border p-4 space-y-3">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="font-medium">{t('photosPanel.title')}</h2>
          <p className="text-xs text-gray-500">{t('photosPanel.desc')}</p>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          {photos && <span className="text-xs text-gray-500">{t('photosPanel.count', { count: fmt.number(count), max: fmt.number(MAX_PHOTOS) })}</span>}
          <Button size="sm" variant="outline" onClick={() => void add()} disabled={busy || full}>
            {busy ? <Loader2 className="w-4 h-4 me-1 animate-spin" aria-hidden /> : <ImagePlus className="w-4 h-4 me-1" aria-hidden />}
            {busy ? t('photosPanel.adding') : t('photosPanel.add')}
          </Button>
        </div>
      </div>

      {loadError ? (
        <div className="text-sm text-red-700 flex items-center gap-2">
          {t('photosPanel.loadError', { error: loadError })}
          <Button size="sm" variant="outline" onClick={() => void load()}>{t('profiles.retry')}</Button>
        </div>
      ) : !photos ? (
        <div className="text-sm text-gray-500">{t('common.loading')}</div>
      ) : photos.length === 0 ? (
        <div className="text-sm text-gray-500">{t('photosPanel.empty')}</div>
      ) : (
        <div className="grid grid-cols-3 sm:grid-cols-5 gap-2">
          {photos.map((photo) => (
            <div key={photo.id} className="space-y-1">
              <Thumbnail photo={photo} onDelete={() => void remove(photo)} />
              {confirmId === photo.id && (
                <div className="flex items-center gap-1 text-xs">
                  <span className="text-gray-600 flex-1">{t('photosPanel.deleteConfirm')}</span>
                  <Button size="sm" variant="destructive" onClick={() => void remove(photo)} disabled={busy}>{t('common.delete')}</Button>
                  <Button size="sm" variant="ghost" onClick={() => setConfirmId(null)}>{t('common.cancel')}</Button>
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
