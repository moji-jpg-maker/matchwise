'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Plus } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useI18n } from '@/i18n';
import { fieldLabel } from '@/lib/field-label';
import { normalizeSearch } from '@/i18n/format';
import type { MessageKey } from '@/i18n/types';
import { FieldDef, FieldKind, kindName, kindOptions } from '@/types/matchmaking';

type SimpleKind = 'bool' | 'number' | 'text' | 'choice' | 'multi_choice';

interface Draft {
  key: string;
  label: string;
  labelFa: string;
  kind: SimpleKind;
  options: string;
  sensitive: boolean;
  required: boolean;
  existing: boolean;
}

const KIND_KEY: Record<ReturnType<typeof kindName>, MessageKey> = {
  text: 'fields.kindText',
  number: 'fields.kindNumber',
  bool: 'fields.kindBool',
  choice: 'fields.kindChoice',
  multi_choice: 'fields.kindMulti',
  records: 'fields.kindRecords',
};

const EMPTY: Draft = { key: '', label: '', labelFa: '', kind: 'text', options: '', sensitive: false, required: false, existing: false };

function buildKind(draft: Draft): FieldKind {
  const options = draft.options.split(/[,،]/).map((o) => o.trim()).filter((o) => o !== '');
  if (draft.kind === 'choice') return { choice: options };
  if (draft.kind === 'multi_choice') return { multi_choice: options };
  return draft.kind;
}

export default function FieldsPage() {
  const router = useRouter();
  const { t, locale } = useI18n();
  const [fields, setFields] = useState<FieldDef[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [filter, setFilter] = useState('');
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      setFields(await invoke<FieldDef[]>('mm_list_fields'));
    } catch (e) {
      setLoadError(`${e}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const edit = (f: FieldDef) => {
    const kind = kindName(f.kind);
    if (kind === 'records') {
      toast.message(t('fields.recordsNotEditable'));
      return;
    }
    setDraft({
      key: f.key,
      label: f.label,
      labelFa: f.label_fa ?? '',
      kind,
      options: kindOptions(f.kind).join(', '),
      sensitive: f.sensitive,
      required: f.required,
      existing: true,
    });
  };

  const save = async () => {
    if (!draft) return;
    setSaving(true);
    try {
      await invoke('mm_save_field', {
        def: {
          key: draft.key.trim(),
          label: draft.label.trim(),
          label_fa: draft.labelFa.trim() || undefined,
          kind: buildKind(draft),
          sensitive: draft.sensitive,
          required: draft.required,
        },
        locale,
      });
      toast.success(t('fields.saved'));
      setDraft(null);
      await load();
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setSaving(false);
    }
  };

  const needle = normalizeSearch(filter);
  const visible = needle
    ? fields.filter((f) => normalizeSearch(f.key).includes(needle) || normalizeSearch(f.label).includes(needle) || normalizeSearch(f.label_fa ?? '').includes(needle))
    : fields;
  const hasOptions = draft?.kind === 'choice' || draft?.kind === 'multi_choice';

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-3xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/profiles')} className="p-2 rounded-lg hover:bg-gray-200" aria-label={t('profiles.back')}>
              <ArrowLeft className="w-5 h-5 rtl-mirror" />
            </button>
            <div>
              <h1 className="text-2xl font-semibold">{t('fields.title')}</h1>
              <p className="text-sm text-gray-500">{t('fields.intro')}</p>
            </div>
          </div>
          <Button onClick={() => setDraft({ ...EMPTY })}>
            <Plus className="w-4 h-4 me-1" /> {t('fields.newField')}
          </Button>
        </div>

        <Input placeholder={t('fields.search')} value={filter} onChange={(e) => setFilter(e.target.value)} />

        {draft && (
          <div className="bg-white rounded-lg border p-4 space-y-3">
            <h2 className="font-medium">{draft.existing ? t('fields.editKey', { key: draft.key }) : t('fields.newField')}</h2>
            <div className="grid grid-cols-2 gap-3">
              <label className="text-sm space-y-1">
                <span className="font-medium">{t('fields.key')}</span>
                <Input
                  value={draft.key}
                  disabled={draft.existing}
                  dir="ltr"
                  placeholder={t('fields.keyPlaceholder')}
                  onChange={(e) => setDraft({ ...draft, key: e.target.value })}
                />
              </label>
              <label className="text-sm space-y-1">
                <span className="font-medium">{t('fields.label')}</span>
                <Input dir="auto" value={draft.label} onChange={(e) => setDraft({ ...draft, label: e.target.value })} />
              </label>
              <label className="text-sm space-y-1">
                <span className="font-medium">{t('fields.labelFa')}</span>
                <Input dir="auto" value={draft.labelFa} onChange={(e) => setDraft({ ...draft, labelFa: e.target.value })} />
              </label>
              <label className="text-sm space-y-1">
                <span className="font-medium">{t('fields.type')}</span>
                <select
                  className="w-full border rounded h-9 px-2 text-sm bg-white"
                  value={draft.kind}
                  onChange={(e) => setDraft({ ...draft, kind: e.target.value as SimpleKind })}
                >
                  <option value="text">{t('fields.kindText')}</option>
                  <option value="number">{t('fields.kindNumber')}</option>
                  <option value="bool">{t('fields.kindBool')}</option>
                  <option value="choice">{t('fields.kindChoice')}</option>
                  <option value="multi_choice">{t('fields.kindMulti')}</option>
                </select>
              </label>
              {hasOptions && (
                <label className="text-sm space-y-1">
                  <span className="font-medium">{t('fields.options')}</span>
                  <Input dir="auto" value={draft.options} onChange={(e) => setDraft({ ...draft, options: e.target.value })} />
                </label>
              )}
            </div>
            <div className="flex gap-6 text-sm">
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={draft.required} onChange={(e) => setDraft({ ...draft, required: e.target.checked })} />
                {t('fields.required')}
              </label>
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={draft.sensitive} onChange={(e) => setDraft({ ...draft, sensitive: e.target.checked })} />
                {t('fields.sensitiveLabel')}
              </label>
            </div>
            <div className="flex gap-2">
              <Button onClick={save} disabled={saving}>{saving ? t('fields.saving') : t('common.save')}</Button>
              <Button variant="ghost" onClick={() => setDraft(null)} disabled={saving}>{t('common.cancel')}</Button>
            </div>
          </div>
        )}

        {loading ? (
          <div className="text-sm text-gray-500">{t('common.loading')}</div>
        ) : loadError ? (
          <div className="bg-white rounded-lg border p-6 text-center space-y-2">
            <p className="text-sm text-red-700">{t('fields.loadError', { error: loadError })}</p>
            <Button variant="outline" onClick={() => void load()}>{t('profiles.retry')}</Button>
          </div>
        ) : (
          <div className="bg-white rounded-lg border divide-y">
            {visible.length === 0 && (
              <div className="p-6 text-center text-sm text-gray-500">
                {fields.length === 0 ? t('fields.none') : t('fields.noMatch')}
              </div>
            )}
            {visible.map((f) => (
              <div key={f.key} className="p-4 flex items-center gap-4">
                <div className="flex-1 min-w-0">
                  <div className="font-medium"><bdi>{fieldLabel(f, locale)}</bdi></div>
                  <div className="text-xs text-gray-500">
                    <bdi dir="ltr">{f.key}</bdi> · {t(KIND_KEY[kindName(f.kind)])}
                    {f.required ? ` · ${t('fields.required')}` : ''}
                    {f.sensitive ? ` · ${t('profile.sensitive')}` : ''}
                  </div>
                </div>
                <Button variant="ghost" size="sm" onClick={() => edit(f)}>{t('fields.edit')}</Button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
