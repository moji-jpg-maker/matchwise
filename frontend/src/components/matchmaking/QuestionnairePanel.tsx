'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { InstrumentView, QuestionnaireDetail, QuestionnaireResponse } from '@/types/matchmaking';

interface AnswerResult {
  response: QuestionnaireResponse;
  completed: boolean;
  not_applied: string[];
}

/** Questionnaires for one person: give them one, enter their answers during an interview, and see the scores with their caveats. */
export function QuestionnairePanel({ profileId, onScores }: { profileId: string; onScores: () => void }) {
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
        invoke<InstrumentView[]>('q_list_instruments', { includeArchived: false }),
        invoke<QuestionnaireResponse[]>('q_profile_responses', { profileId }),
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
  }, [profileId]);
  useEffect(() => { void load(); }, [load]);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try { await fn(); } catch (e) { toast.error(`${e}`); } finally { setBusy(false); }
  };

  const assign = () =>
    run(async () => {
      const r = await invoke<{ response: QuestionnaireResponse; notified: boolean }>('q_assign', { profileId, instrumentKey: pick });
      toast.success(r.notified ? 'Given to them, and they were told on Telegram' : 'Given to them');
      await load();
    });

  const openForm = (id: number) => run(async () => setDetail(await invoke<QuestionnaireDetail>('q_get_response', { id })));

  const answer = (itemId: string, value: number) =>
    run(async () => {
      if (!detail) return;
      const r = await invoke<AnswerResult>('q_answer', { id: detail.response.id, itemId, value });
      setDetail({ ...detail, response: r.response, answers: { ...detail.answers, [itemId]: value } });
      if (r.completed) {
        toast.success('Questionnaire completed: scores saved to the profile');
        if (r.not_applied.length) toast.warning(`Kept the existing value for: ${r.not_applied.join(', ')}`);
        onScores();
        setDetail(null);
      }
      await load();
    });

  const remove = (r: QuestionnaireResponse, clear: boolean) =>
    run(async () => {
      await invoke('q_delete_response', { id: r.id, clearScores: clear });
      await load();
      onScores();
    });

  return (
    <div className="bg-white rounded-lg border">
      <div className="p-4 border-b">
        <h2 className="font-medium">Questionnaires</h2>
        <p className="text-xs text-gray-500">Scores become profile fields marked “from a questionnaire”. They describe self-reported tendencies, not a clinical assessment.</p>
      </div>

      <div className="p-4 flex flex-wrap items-center gap-2 border-b">
        <select className="border rounded-md px-2 py-1.5 text-sm bg-white" value={pick} onChange={(e) => setPick(e.target.value)}>
          {instruments.map((i) => <option key={i.instrument.key} value={i.instrument.key}>{i.instrument.name}</option>)}
        </select>
        <Button size="sm" disabled={busy || !pick} onClick={assign}>Give to this person</Button>
        <span className="text-xs text-gray-500">They can answer on Telegram (/questionnaires) if linked, or you can enter their answers below.</span>
      </div>

      {loading && <p className="p-4 text-sm text-gray-500">Loading…</p>}
      {loadError && !loading && (
        <div className="p-4 text-sm text-red-700 flex items-center gap-2">
          Could not load questionnaires: {loadError}
          <Button size="sm" variant="outline" onClick={() => { setLoading(true); void load(); }}>Retry</Button>
        </div>
      )}
      {!loading && !loadError && responses.length === 0 && <p className="p-4 text-sm text-gray-500">No questionnaires yet.</p>}
      <div className="divide-y">
        {responses.map((r) => (
          <div key={r.id} className="p-4 space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-medium text-sm">{r.instrument_name}</span>
              <span className="text-xs text-gray-500">v{r.instrument_version}</span>
              <span className={`text-xs px-2 py-0.5 rounded ${r.status === 'complete' ? 'bg-green-100 text-green-800' : 'bg-blue-100 text-blue-800'}`}>
                {r.status === 'complete' ? 'completed' : `${r.answered} of ${r.total} answered`}
              </span>
              <span className="text-xs px-2 py-0.5 rounded bg-gray-100">{r.evidence_label}</span>
              <div className="flex-1" />
              {r.status !== 'complete' && <Button size="sm" variant="outline" disabled={busy} onClick={() => openForm(r.id)}>Enter answers</Button>}
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => remove(r, r.status === 'complete')}>{r.status === 'complete' ? 'Delete and clear scores' : 'Delete'}</Button>
            </div>
            {r.scores.length > 0 && (
              <div className="text-sm space-y-1">
                <div className="flex flex-wrap gap-3">{r.scores.map((s) => <span key={s.subscale} className="px-2 py-1 bg-gray-50 border rounded">{s.label}: <b>{Math.round(s.value * 100) / 100}</b></span>)}</div>
                {r.interpretation && <p className="text-xs text-gray-600">{r.interpretation}</p>}
                <details className="text-xs text-gray-500"><summary className="cursor-pointer">What this cannot tell you</summary><p className="whitespace-pre-line mt-1">{r.limitations}</p></details>
              </div>
            )}
          </div>
        ))}
      </div>

      {detail && (
        <div className="border-t p-4 space-y-3 bg-gray-50">
          <div className="flex items-center gap-2">
            <h3 className="font-medium flex-1">{detail.instrument.name}: {Object.keys(detail.answers).length} of {detail.instrument.items.length}</h3>
            <Button size="sm" variant="ghost" onClick={() => setDetail(null)}>Close (progress is saved)</Button>
          </div>
          {detail.instrument.description && <p className="text-sm text-gray-600">{detail.instrument.description}</p>}
          <ol className="space-y-3">
            {detail.instrument.items.map((it, n) => (
              <li key={it.id} className="text-sm">
                <div className="mb-1">{n + 1}. {it.text}</div>
                <div className="flex flex-wrap gap-1">
                  {detail.instrument.scale.labels.map((label, k) => {
                    const v = detail.instrument.scale.min + k;
                    return (
                      <button key={v} disabled={busy} onClick={() => answer(it.id, v)} title={label} className={`px-2.5 py-1 rounded border text-xs ${detail.answers[it.id] === v ? 'bg-blue-600 text-white border-blue-600' : 'bg-white hover:bg-gray-100'}`}>
                        {v}
                      </button>
                    );
                  })}
                </div>
              </li>
            ))}
          </ol>
          <p className="text-xs text-gray-500">Scale: {detail.instrument.scale.labels.map((l, k) => `${detail.instrument.scale.min + k} = ${l}`).join('; ')}</p>
        </div>
      )}
    </div>
  );
}
