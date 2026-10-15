'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { useI18n } from '@/i18n';
import { InstrumentView, QuestionnaireDetail, QuestionnaireResponse } from '@/types/matchmaking';

interface AnswerResult {
  response: QuestionnaireResponse;
  completed: boolean;
  not_applied: string[];
}

/** Questionnaires for one person: give them one, enter their answers during an interview, and see the scores with their caveats. */
export function QuestionnairePanel({ profileId, onScores }: { profileId: string; onScores: () => void }) {
  const { t, fmt, locale } = useI18n();
  const [instruments, setInstruments] = useState<InstrumentView[]>([]);
  const [responses, setResponses] = useState<QuestionnaireResponse[]>([]);
  const [pick, setPick] = useState('');
  const [detail, setDetail] = useState<QuestionnaireDetail | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoadError(null);
    try {
      const [ins, res] = await Promise.all([
        invoke<InstrumentView[]>('q_list_instruments', { includeArchived: false, locale }),
        invoke<QuestionnaireResponse[]>('q_profile_responses', { profileId, locale }),
      ]);
      const latest = ins.filter((i) => i.is_latest);
      setInstruments(latest);
      setResponses(res);
      setPick((p) => p || latest[0]?.instrument.key || '');
    } catch (e) {
      setLoadError(`${e}`);
      toast.error(`${e}`);
    } finally {
      setLoading(false);
    }
  }, [profileId, locale]);
  useEffect(() => { void load(); }, [load]);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try { await fn(); } catch (e) { toast.error(`${e}`); } finally { setBusy(false); }
  };

  const assign = () =>
    run(async () => {
      const r = await invoke<{ response: QuestionnaireResponse; notified: boolean }>('q_assign', { profileId, instrumentKey: pick, locale });
      toast.success(r.notified ? t('questionnairePanel.assignedNotified') : t('questionnairePanel.assigned'));
      await load();
    });

  const openForm = (id: number) => run(async () => setDetail(await invoke<QuestionnaireDetail>('q_get_response', { id, locale })));

  const answer = (itemId: string, value: number) =>
    run(async () => {
      if (!detail) return;
      const r = await invoke<AnswerResult>('q_answer', { id: detail.response.id, itemId, value, locale });
      setDetail({ ...detail, response: r.response, answers: { ...detail.answers, [itemId]: value } });
      if (r.completed) {
        toast.success(t('questionnairePanel.completed'));
        if (r.not_applied.length) toast.warning(t('questionnairePanel.notApplied', { fields: r.not_applied.join(t('questionnairePanel.listSeparator')) }));
        onScores();
        setDetail(null);
      }
      await load();
    });

  const remove = (r: QuestionnaireResponse, clear: boolean) =>
    run(async () => {
      await invoke('q_delete_response', { id: r.id, clearScores: clear, locale });
      await load();
      onScores();
    });

  return (
    <div className="bg-white rounded-lg border">
      <div className="p-4 border-b">
        <h2 className="font-medium">{t('questionnairePanel.title')}</h2>
        <p className="text-xs text-gray-500">{t('questionnairePanel.note')}</p>
      </div>

      <div className="p-4 flex flex-wrap items-center gap-2 border-b">
        <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={pick} onChange={(e) => setPick(e.target.value)}>
          {instruments.map((i) => <option key={i.instrument.key} value={i.instrument.key}>{i.instrument.name}</option>)}
        </select>
        <Button size="sm" disabled={busy || !pick} onClick={assign}>{t('questionnairePanel.give')}</Button>
        <span className="text-xs text-gray-500">{t('questionnairePanel.telegramHint')}</span>
      </div>

      {loading && <p className="p-4 text-sm text-gray-500">{t('common.loading')}</p>}
      {loadError && !loading && (
        <div className="p-4 text-sm text-red-700 flex items-center gap-2">
          {t('questionnairePanel.loadError', { error: loadError })}
          <Button size="sm" variant="outline" onClick={() => { setLoading(true); void load(); }}>{t('profiles.retry')}</Button>
        </div>
      )}
      {!loading && !loadError && responses.length === 0 && <p className="p-4 text-sm text-gray-500">{t('questionnairePanel.empty')}</p>}
      <div className="divide-y">
        {responses.map((r) => (
          <div key={r.id} className="p-4 space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-medium text-sm"><bdi dir="auto">{r.instrument_name}</bdi></span>
              <span className="text-xs text-gray-500">{t('questionnairePanel.versionTag', { version: fmt.number(r.instrument_version) })}</span>
              <span className={`text-xs px-2 py-0.5 rounded ${r.status === 'complete' ? 'bg-green-100 text-green-800' : 'bg-blue-100 text-blue-800'}`}>
                {r.status === 'complete' ? t('questionnairePanel.done') : t('questionnairePanel.progress', { answered: fmt.number(r.answered), total: fmt.number(r.total) })}
              </span>
              <span className="text-xs px-2 py-0.5 rounded bg-gray-100"><bdi dir="auto">{r.evidence_label}</bdi></span>
              <div className="flex-1" />
              {r.status !== 'complete' && <Button size="sm" variant="outline" disabled={busy} onClick={() => openForm(r.id)}>{t('questionnairePanel.enterAnswers')}</Button>}
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => remove(r, r.status === 'complete')}>{r.status === 'complete' ? t('questionnairePanel.deleteClear') : t('common.delete')}</Button>
            </div>
            {r.scores.length > 0 && (
              <div className="text-sm space-y-1">
                <div className="flex flex-wrap gap-3">{r.scores.map((s) => <span key={s.subscale} className="px-2 py-1 bg-gray-50 border rounded"><bdi dir="auto">{s.label}</bdi>: <b>{fmt.number(s.value, { maximumFractionDigits: 2 })}</b></span>)}</div>
                {r.interpretation && <p dir="auto" className="text-xs text-gray-600">{r.interpretation}</p>}
                <details className="text-xs text-gray-500"><summary className="cursor-pointer">{t('questionnairePanel.limitations')}</summary><p dir="auto" className="whitespace-pre-line mt-1">{r.limitations}</p></details>
              </div>
            )}
          </div>
        ))}
      </div>

      {detail && (
        <div className="border-t p-4 space-y-3 bg-gray-50">
          <div className="flex items-center gap-2">
            <h3 className="font-medium flex-1"><bdi dir="auto">{t('questionnairePanel.formTitle', { name: detail.instrument.name, answered: fmt.number(Object.keys(detail.answers).length), total: fmt.number(detail.instrument.items.length) })}</bdi></h3>
            <Button size="sm" variant="ghost" onClick={() => setDetail(null)}>{t('questionnairePanel.close')}</Button>
          </div>
          {detail.instrument.description && <p dir="auto" className="text-sm text-gray-600">{detail.instrument.description}</p>}
          <ol className="space-y-3">
            {detail.instrument.items.map((it, n) => (
              <li key={it.id} className="text-sm">
                <div className="mb-1">{fmt.number(n + 1)}. <bdi dir="auto">{it.text}</bdi></div>
                <div className="flex flex-wrap gap-1">
                  {detail.instrument.scale.labels.map((label, k) => {
                    const v = detail.instrument.scale.min + k;
                    return (
                      <button key={v} disabled={busy} onClick={() => answer(it.id, v)} title={label} dir="ltr" className={`px-2.5 py-1 rounded border text-xs ${detail.answers[it.id] === v ? 'bg-blue-600 text-white border-blue-600' : 'bg-white hover:bg-gray-100'}`}>
                        {fmt.number(v)}
                      </button>
                    );
                  })}
                </div>
              </li>
            ))}
          </ol>
          <p className="text-xs text-gray-500">{t('questionnairePanel.scaleLine', { scale: detail.instrument.scale.labels.map((label, k) => t('questionnairePanel.scaleEntry', { value: fmt.number(detail.instrument.scale.min + k), label })).join(t('questionnairePanel.scaleSeparator')) })}</p>
        </div>
      )}
    </div>
  );
}
