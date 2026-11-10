'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useI18n } from '@/i18n';
import type { ExtractionPreset, ExtractionPresetField, ExtractionPresetsView } from '@/types/matchmaking';

type Draft = {
  id: string | null;
  name: string;
  fields: Set<string>;
  withPreferences: boolean;
  isDefault: boolean;
};

const newDraft = (fields: ExtractionPresetField[]): Draft => ({
  id: null,
  name: '',
  fields: new Set(fields.filter((field) => !field.sensitive).map((field) => field.key)),
  withPreferences: false,
  isDefault: false,
});

const draftOf = (preset: ExtractionPreset): Draft => ({
  id: preset.id,
  name: preset.name,
  fields: new Set(preset.fields),
  withPreferences: preset.with_preferences,
  isDefault: preset.is_default,
});

/** Saves which fields a reading run asks for. Local settings only: a preset never names a value. */
export function AiPresetsEditor() {
  const { t, fmt, locale } = useI18n();
  const [view, setView] = useState<ExtractionPresetsView | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);

  const apply = useCallback((next: ExtractionPresetsView) => {
    setView(next);
    setDraft((current) => (current ? current : null));
  }, []);

  const load = useCallback(async () => {
    try {
      apply(await invoke<ExtractionPresetsView>('mm_extraction_presets', { locale }));
    } catch (e) {
      toast.error(`${e}`);
    }
  }, [apply, locale]);
  useEffect(() => { void load(); }, [load]);

  const run = async (fn: () => Promise<ExtractionPresetsView>, ok: string) => {
    setBusy(true);
    try {
      apply(await fn());
      toast.success(ok);
      return true;
    } catch (e) {
      toast.error(`${e}`);
      return false;
    } finally {
      setBusy(false);
    }
  };

  if (!view) return <div className="text-sm text-gray-500">{t('ai.loading')}</div>;

  const fieldList = view.fields;
  const toggle = (key: string) => {
    setDraft((current) => {
      if (!current) return current;
      const next = new Set(current.fields);
      if (next.has(key)) next.delete(key); else next.add(key);
      return { ...current, fields: next };
    });
  };

  const save = async () => {
    if (!draft) return;
    const saved = await run(
      () => invoke<ExtractionPresetsView>('mm_save_extraction_preset', {
        id: draft.id,
        name: draft.name,
        fields: [...draft.fields],
        withPreferences: draft.withPreferences,
        isDefault: draft.isDefault,
        locale,
      }),
      t('ai.presetSaved'),
    );
    if (saved) setDraft(null);
  };

  const remove = async (preset: ExtractionPreset) => {
    if (!window.confirm(t('ai.presetDeleteConfirm', { name: preset.name }))) return;
    await run(() => invoke<ExtractionPresetsView>('mm_delete_extraction_preset', { id: preset.id, locale }), t('ai.presetDeleted'));
    setDraft((current) => (current && current.id === preset.id ? null : current));
  };

  return (
    <div className="bg-white rounded-lg border p-4 space-y-3">
      <div>
        <h2 className="font-medium">{t('ai.presetsTitle')}</h2>
        <p className="text-xs text-gray-500">{t('ai.presetsIntro')}</p>
      </div>

      {view.presets.length === 0 && !draft && <p className="text-sm text-gray-500">{t('ai.presetEmpty')}</p>}
      <ul className="space-y-2">
        {view.presets.map((preset) => (
          <li key={preset.id} className="flex flex-wrap items-center gap-2 text-sm border rounded p-2">
            <span className="font-medium" dir="auto">{preset.name}</span>
            {preset.is_default && <span className="text-xs px-1.5 py-0.5 rounded bg-green-100 text-green-800">{t('ai.presetDefault')}</span>}
            {preset.with_preferences && <span className="text-xs px-1.5 py-0.5 rounded bg-amber-100 text-amber-800">{t('ai.presetPreferences')}</span>}
            <span className="text-xs text-gray-500">{t('ai.presetFieldCount', { count: fmt.number(preset.fields.length) })}</span>
            <span className="flex-1" />
            <Button size="sm" variant="outline" disabled={busy} onClick={() => setDraft(draftOf(preset))}>{t('ai.presetEdit')}</Button>
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => void remove(preset)}>{t('ai.presetDelete')}</Button>
          </li>
        ))}
      </ul>

      {!draft && (
        <Button size="sm" variant="outline" disabled={busy} onClick={() => setDraft(newDraft(fieldList))}>{t('ai.presetNew')}</Button>
      )}

      {draft && (
        <div className="border rounded p-3 space-y-3">
          <div className="flex flex-wrap items-end gap-2">
            <label className="space-y-1 flex-1 min-w-48">
              <span className="block text-xs text-gray-500">{t('ai.presetName')}</span>
              <Input value={draft.name} placeholder={t('ai.presetNamePlaceholder')} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
            </label>
            <Button size="sm" disabled={busy || draft.name.trim() === '' || draft.fields.size === 0} onClick={() => void save()}>{t('ai.presetSave')}</Button>
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => setDraft(null)}>{t('ai.presetCancel')}</Button>
          </div>

          <div className="space-y-2">
            <div className="flex items-center gap-2">
              <span className="text-xs text-gray-500 flex-1">{t('ai.presetFields')}</span>
              <Button size="sm" variant="ghost" onClick={() => setDraft({ ...draft, fields: new Set(fieldList.map((field) => field.key)) })}>{t('ai.presetAll')}</Button>
              <Button size="sm" variant="ghost" onClick={() => setDraft({ ...draft, fields: new Set() })}>{t('ai.presetNone')}</Button>
            </div>
            <div className="max-h-64 overflow-auto grid grid-cols-1 sm:grid-cols-2 gap-1">
              {fieldList.map((field) => (
                <label key={field.key} className="flex items-center gap-2 text-sm">
                  <input type="checkbox" checked={draft.fields.has(field.key)} onChange={() => toggle(field.key)} />
                  <bdi dir="auto">{field.label}</bdi>
                  {field.sensitive && <span className="text-xs text-amber-700">{t('ai.presetSensitive')}</span>}
                </label>
              ))}
            </div>
          </div>

          <label className="flex items-start gap-2 text-sm">
            <input type="checkbox" className="mt-1" checked={draft.withPreferences} onChange={(e) => setDraft({ ...draft, withPreferences: e.target.checked })} />
            <span>{t('ai.presetPreferences')}</span>
          </label>
          <label className="flex items-start gap-2 text-sm">
            <input type="checkbox" className="mt-1" checked={draft.isDefault} onChange={(e) => setDraft({ ...draft, isDefault: e.target.checked })} />
            <span>{t('ai.presetDefault')}<br /><span className="text-xs text-gray-500">{t('ai.presetDefaultHint')}</span></span>
          </label>
        </div>
      )}
    </div>
  );
}
