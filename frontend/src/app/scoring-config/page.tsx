'use client';

import React, { Suspense, useCallback, useEffect, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useI18n } from '@/i18n';
import { ScoringConfig, ScoringConfigView } from '@/types/matchmaking';

/** An empty box is not a zero: it means the matchmaker has not said, so it is never valid. */
function num(value: string): number {
  return value.trim() === '' ? NaN : Number(value);
}

function bad(message: string, v: number): string | null {
  return Number.isFinite(v) ? null : message;
}

function Editor() {
  const { t, fmt } = useI18n();
  const router = useRouter();
  const idParam = useSearchParams().get('id');
  const isNew = idParam === 'new' || !idParam;

  const [meta, setMeta] = useState<ScoringConfigView | null>(null);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [strong, setStrong] = useState('75');
  const [mixed, setMixed] = useState('50');
  const [prior, setPrior] = useState('50');
  const [note, setNote] = useState('');
  const [issues, setIssues] = useState<string[]>([]);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);

  const apply = useCallback((v: ScoringConfigView) => {
    setMeta(v);
    setName(v.name);
    setDescription(v.description);
    setStrong(String(v.definition.strong_threshold));
    setMixed(String(v.definition.mixed_threshold));
    setPrior(String(v.definition.prior_score));
    setNote('');
    setDirty(false);
  }, []);

  const load = useCallback(
    async (version?: number) => {
      try {
        if (isNew) {
          setMeta(null);
          setName('');
          setDescription('');
          setStrong('75');
          setMixed('50');
          setPrior('50');
          setNote('');
          setDirty(false);
        } else {
          apply(await invoke<ScoringConfigView>('mm_get_scoring_config', { id: idParam, version: version ?? null }));
          if (version) setDirty(true); // viewing an old version: saving makes it the new current version
        }
      } catch (e) {
        toast.error(t('scoringConfig.loadError', { error: `${e}` }));
      }
    },
    [apply, idParam, isNew, t]
  );

  useEffect(() => { void load(); }, [load]);

  const definition = useCallback(
    (): ScoringConfig => ({ strong_threshold: num(strong), mixed_threshold: num(mixed), prior_score: num(prior) }),
    [strong, mixed, prior]
  );

  // A half-typed number cannot be sent (it would not even survive the trip), so this is checked here first; the
  // backend still re-checks on save, and what it says about the rest is what is shown.
  const localIssues = [
    bad(t('scoringConfig.mustBeNumber', { label: t('scoringConfig.strongCutoff') }), num(strong)),
    bad(t('scoringConfig.mustBeNumber', { label: t('scoringConfig.mixedCutoff') }), num(mixed)),
    bad(t('scoringConfig.mustBeNumber', { label: t('scoringConfig.neutralBaseline') }), num(prior)),
  ].filter(
    (m): m is string => m !== null
  );
  const shown = [...localIssues, ...issues];

  // live validation (debounced); the backend is the single source of truth for what is valid
  useEffect(() => {
    if (localIssues.length > 0) {
      setIssues([]);
      return;
    }
    const timer = setTimeout(async () => {
      try {
        setIssues(await invoke<string[]>('mm_validate_scoring_config', { definition: definition() }));
      } catch {
        /* validation is advisory; saving re-checks */
      }
    }, 400);
    return () => clearTimeout(timer);
  }, [definition, localIssues.length]);

  const save = async () => {
    setSaving(true);
    try {
      const v = await invoke<ScoringConfigView>('mm_save_scoring_config', {
        id: isNew ? null : idParam,
        name: name.trim(),
        description,
        definition: definition(),
        note: note || null,
      });
      toast.success(t('scoringConfig.saved', { version: fmt.number(v.current_version) }));
      if (isNew) router.replace(`/scoring-config?id=${encodeURIComponent(v.id)}`);
      else apply(v);
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setSaving(false);
    }
  };

  const touch = () => setDirty(true);
  const blocked = shown.length > 0;

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/scoring')} className="p-2 rounded-lg hover:bg-gray-200" aria-label={t('scoringConfig.back')}>
              <ArrowLeft className="w-5 h-5 rtl-mirror" />
            </button>
            <div>
              <h1 className="text-2xl font-semibold">{isNew ? t('scoringConfig.newTitle') : name ? <bdi>{name}</bdi> : t('scoringConfig.fallbackTitle')}</h1>
              <p className="text-sm text-gray-500">
                {t('scoringConfig.intro')}
              </p>
            </div>
            {meta && (
              <span className="text-sm text-gray-500">
                {meta.version !== meta.current_version
                  ? t('scoringConfig.showingCurrent', { version: fmt.number(meta.version), current: fmt.number(meta.current_version) })
                  : t('scoringConfig.showing', { version: fmt.number(meta.version) })}
              </span>
            )}
          </div>
          <Button onClick={save} disabled={saving || blocked || (!dirty && !isNew) || name.trim() === ''}>
            {saving ? t('scoringConfig.saving') : isNew ? t('scoringConfig.create') : t('scoringConfig.saveVersion')}
          </Button>
        </div>

        {meta && meta.version !== meta.current_version && (
          <div className="bg-amber-50 border border-amber-200 rounded-lg p-3 text-sm">
            {t('scoringConfig.olderVersion')}
          </div>
        )}

        {blocked && (
          <div className="bg-red-50 border border-red-200 rounded-lg p-3 text-sm text-red-800">
            <div className="font-medium">{t('scoringConfig.notSaved')}</div>
            <ul className="list-disc ms-5">
              {shown.map((m, i) => (
                <li key={i}>{m}</li>
              ))}
            </ul>
          </div>
        )}

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <div className="grid grid-cols-3 gap-3 items-center">
            <label className="text-sm font-medium">{t('scoringConfig.name')}</label>
            <Input className="col-span-2" dir="auto" value={name} onChange={(e) => { setName(e.target.value); touch(); }} />
            <label className="text-sm font-medium">{t('scoringConfig.description')}</label>
            <Input className="col-span-2" dir="auto" value={description} onChange={(e) => { setDescription(e.target.value); touch(); }} />
          </div>

          <div className="border-t pt-3">
            <div className="text-sm font-medium mb-2">{t('scoringConfig.reading')}</div>
            <div className="grid grid-cols-3 gap-3 items-center">
              <label className="text-sm font-medium" title={t('scoringConfig.strongTip')}>
                {t('scoringConfig.strongField')}
              </label>
              <Input className="w-28" dir="ltr" type="number" step="0.1" value={strong} onChange={(e) => { setStrong(e.target.value); touch(); }} />
              <label className="text-sm font-medium" title={t('scoringConfig.mixedTip')}>
                {t('scoringConfig.mixedField')}
              </label>
              <Input className="w-28" dir="ltr" type="number" step="0.1" value={mixed} onChange={(e) => { setMixed(e.target.value); touch(); }} />
              <label className="text-sm font-medium" title={t('scoringConfig.baselineTip')}>
                {t('scoringConfig.baselineField')}
              </label>
              <Input className="w-28" dir="ltr" type="number" step="0.1" value={prior} onChange={(e) => { setPrior(e.target.value); touch(); }} />
            </div>
            <p className="text-xs text-gray-500 mt-2">
              {t('scoringConfig.baselineNote', { prior: prior.trim() === '' ? t('scoringConfig.neutralValue') : fmt.number(Number(prior)) })}
            </p>
          </div>
        </div>

        {(dirty || isNew) && (
          <div className="bg-white rounded-lg border p-4">
            <label className="text-sm font-medium">{t('scoringConfig.noteLabel')}</label>
            <Input className="mt-1" dir="auto" value={note} onChange={(e) => setNote(e.target.value)} />
          </div>
        )}

        {meta && (
          <div className="bg-white rounded-lg border p-4">
            <h2 className="font-medium mb-1">{t('scoringConfig.historyTitle')}</h2>
            <p className="text-xs text-gray-500 mb-2">
              {t('scoringConfig.historyIntro')}
            </p>
            <ul className="space-y-1 text-sm">
              {meta.versions.map((v) => (
                <li key={v.version} className="flex items-center gap-3">
                  <span className="w-10 font-medium">{t('scoringConfig.versionLabel', { version: fmt.number(v.version) })}</span>
                  <span className="text-gray-500 w-44">{fmt.dateTime(v.created_at)}</span>
                  <span className="flex-1 truncate" dir="auto">{v.note || ''}</span>
                  {v.version !== meta.version && (
                    <button className="text-blue-600 hover:underline" onClick={() => load(v.version)}>
                      {t('scoringConfig.view')}
                    </button>
                  )}
                  {v.version === meta.version && <span className="text-xs text-gray-500">{t('scoringConfig.shownTag')}</span>}
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </div>
  );
}

export default function ScoringConfigPage() {
  const { t } = useI18n();
  const fallbackLoading = t('common.loading');
  return (
    <Suspense fallback={<div className="p-6 text-gray-500">{fallbackLoading}</div>}>
      <Editor />
    </Suspense>
  );
}
