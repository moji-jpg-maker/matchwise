'use client';

import React, { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, FileUp, ListOrdered, CheckCheck, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { IntakeAcceptReport, IntakeBatchReport } from '@/types/matchmaking';
import { useI18n } from '@/i18n';
import type { Translate } from '@/lib/conditions';

const readFile = async (file: File, t: Translate): Promise<{ text: string | null; entries: string[] | null }> => {
  const raw = await file.text();
  if (file.name.toLowerCase().endsWith('.json')) {
    // The file is already split into entries: hand the strings straight to the command so it is not split twice.
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch (e) {
      throw new Error(t('intake.invalidJson', { error: `${e}` }));
    }
    if (!Array.isArray(parsed) || parsed.some((x) => typeof x !== 'string')) {
      throw new Error(t('intake.jsonShape'));
    }
    return { text: null, entries: parsed as string[] };
  }
  return { text: raw, entries: null };
};

const countEntries = (corpus: string, entries: string[] | null): number => {
  if (entries) return entries.filter((e) => e.trim() !== '').length;
  const blocks = /^\s*-{3,}\s*$/m.test(corpus) ? corpus.split(/^\s*-{3,}\s*$/m) : corpus.split(/\n\s*\n/);
  return blocks.filter((b) => b.trim() !== '').length;
};

export default function IntakePage() {
  const router = useRouter();
  const { t, fmt } = useI18n();
  const [target, setTarget] = useState('');
  const [corpus, setCorpus] = useState('');
  const [entries, setEntries] = useState<string[] | null>(null);
  const [fileName, setFileName] = useState<string | null>(null);
  const [batchId, setBatchId] = useState<string | null>(null);
  const [report, setReport] = useState<IntakeBatchReport | null>(null);
  const [accepted, setAccepted] = useState<IntakeAcceptReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [phase, setPhase] = useState<'idle' | 'reading' | 'accepting'>('idle');
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    if (phase === 'idle') {
      setElapsed(0);
      return;
    }
    const started = Date.now();
    const timer = setInterval(() => setElapsed(Math.floor((Date.now() - started) / 1000)), 1000);
    return () => clearInterval(timer);
  }, [phase]);

  useEffect(() => {
    let saved: string | null = null;
    try {
      saved = window.localStorage.getItem('intake:lastBatch');
    } catch {
      saved = null;
    }
    if (!saved) return;
    const id = saved;
    invoke<IntakeBatchReport>('ai_intake_report', { batchId: id })
      .then((r) => {
        setBatchId(id);
        setReport(r);
      })
      .catch(() => {
        try {
          window.localStorage.removeItem('intake:lastBatch');
        } catch {}
      });
  }, []);

  const loadFile = async (file: File) => {
    try {
      const r = await readFile(file, t);
      setEntries(r.entries);
      setCorpus(r.text ?? '');
      setFileName(file.name);
      setReport(null);
      setAccepted(null);
      setBatchId(null);
      toast.success(t('intake.loadedFile', { name: file.name }));
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  const run = async () => {
    if (!corpus.trim() && !entries?.length) {
      toast.error(t('intake.emptyCorpus'));
      return;
    }
    setBusy(true);
    setPhase('reading');
    setAccepted(null);
    try {
      const id = `intake-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
      const r = await invoke<IntakeBatchReport>('ai_intake_batch', {
        target: target.trim() || null,
        targetText: entries ? null : corpus,
        entries: entries ?? null,
        batchId: id,
      });
      setBatchId(id);
      setReport(r);
      try {
        window.localStorage.setItem('intake:lastBatch', id);
      } catch {}
      if (!r.parsed) toast.message(t('intake.nothingRead'), { description: r.note ?? undefined });
      else toast.success(t('intake.createdToast', { created: fmt.number(r.created), suggestions: fmt.number(r.suggestions) }));
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setBusy(false);
      setPhase('idle');
    }
  };

  const acceptAll = async () => {
    if (!batchId) return;
    setBusy(true);
    setPhase('accepting');
    try {
      const r = await invoke<IntakeAcceptReport>('ai_intake_accept_all', { batchId });
      setAccepted(r);
      toast.success(t('intake.acceptedToast', { accepted: fmt.number(r.accepted), skipped: fmt.number(r.skipped) }));
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setBusy(false);
      setPhase('idle');
    }
  };

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-3xl mx-auto p-6 space-y-5">
        <div className="flex items-center gap-3">
          <button onClick={() => router.push('/profiles')} className="p-2 rounded-lg hover:bg-gray-200" aria-label={t('profiles.back')}>
            <ArrowLeft className="w-5 h-5 rtl-mirror" />
          </button>
          <div>
            <h1 className="text-2xl font-semibold">{t('intake.title')}</h1>
            <p className="text-sm text-gray-500">{t('intake.intro')}</p>
          </div>
        </div>

        <div className="bg-white rounded-lg border p-4 space-y-2">
          <div className="text-sm font-medium">{t('intake.targetTitle')}</div>
          <p className="text-xs text-gray-500">{t('intake.targetDesc')}</p>
          <Textarea dir="auto" value={target} onChange={(e) => setTarget(e.target.value)} placeholder={t('intake.targetPlaceholder')} />
        </div>

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <div className="flex items-center justify-between">
            <div>
              <div className="text-sm font-medium">{t('intake.corpusTitle')}</div>
              <p className="text-xs text-gray-500">{t('intake.corpusDesc')}</p>
            </div>
            <label className="flex items-center gap-2 text-sm cursor-pointer">
              <input
                type="file"
                accept=".txt,.json,text/plain,application/json"
                className="hidden"
                onChange={(e) => { const f = e.target.files?.[0]; if (f) void loadFile(f); e.target.value = ''; }}
              />
              <span className="inline-flex items-center gap-1 px-3 py-1.5 border rounded-md hover:bg-gray-50">
                <FileUp className="w-4 h-4" /> {fileName ? t('intake.replaceFile') : t('intake.loadFile')}
              </span>
            </label>
          </div>

          {fileName && <div className="text-xs text-gray-500">{entries ? t('intake.loadedEntries', { name: fileName, count: fmt.number(entries.length) }) : t('intake.loadedFile', { name: fileName })}</div>}

          <Textarea
            dir="auto"
            value={corpus}
            onChange={(e) => { setCorpus(e.target.value); setEntries(null); setFileName(null); }}
            placeholder={t('intake.corpusPlaceholder')}
            className="min-h-[180px] font-mono text-xs"
          />

          <div className="flex flex-wrap gap-2">
            <Button onClick={run} disabled={busy}>{busy ? t('intake.working') : t('intake.create')}</Button>
            {report?.target_profile_id && (
              <Button variant="outline" onClick={() => router.push(`/profile?id=${encodeURIComponent(report.target_profile_id!)}`)}>
                <ListOrdered className="w-4 h-4 me-1" /> {t('intake.showRanked')}
              </Button>
            )}
            {batchId && report?.parsed && (
              <Button variant="outline" onClick={acceptAll} disabled={busy}>
                <CheckCheck className="w-4 h-4 me-1" /> {t('intake.acceptAll')}
              </Button>
            )}
          </div>
        </div>

        {phase !== 'idle' && (
          <div className="bg-white rounded-lg border p-4 space-y-2" role="status" aria-live="polite">
            <div className="flex items-center gap-2 text-sm font-medium">
              <Loader2 className="w-4 h-4 animate-spin" />
              {phase === 'reading'
                ? t(countEntries(corpus, entries) === 1 ? 'intake.readingOne' : 'intake.readingMany', { count: fmt.number(countEntries(corpus, entries)) })
                : t('intake.accepting')}
              <span className="text-gray-500 font-normal">{t('intake.seconds', { count: fmt.number(elapsed) })}</span>
            </div>
            <div className="h-1.5 bg-gray-100 rounded overflow-hidden">
              <div className="h-full w-1/3 bg-blue-500 rounded animate-pulse" />
            </div>
            <p className="text-xs text-gray-500">
              {phase === 'reading'
                ? t('intake.readingHint')
                : t('intake.acceptingHint')}
            </p>
          </div>
        )}

        {report && (
          <div className="bg-white rounded-lg border">
            <div className="p-4 border-b">
              <div className="font-medium">{t('intake.result')}</div>
              <div className="text-sm text-gray-500">
                {t('intake.summary', { created: fmt.number(report.created), dropped: fmt.number(report.dropped), suggestions: fmt.number(report.suggestions) })}
              </div>
              {!report.parsed && report.note && (
                <div className="mt-1 text-sm text-amber-800 bg-amber-50 rounded p-2">{report.note}</div>
              )}
            </div>
            <div className="divide-y">
              {report.entries.map((e) => (
                <div key={e.index} className="p-3 flex items-start gap-3 text-sm">
                  <span className="text-gray-400 w-8 shrink-0">#{fmt.number(e.index)}</span>
                  <div className="flex-1 min-w-0">
                    <div className="font-medium truncate"><bdi>{e.label}</bdi></div>
                    {e.reason && <div className="text-xs text-amber-700">{e.reason}</div>}
                  </div>
                  <span className={`text-xs px-2 py-0.5 rounded shrink-0 ${e.outcome === 'dropped' ? 'bg-red-100 text-red-800' : e.outcome === 'parsed' ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-700'}`}>
                    {e.outcome === 'parsed' ? t('intake.outcomeParsed') : e.outcome === 'dropped' ? t('intake.outcomeDropped') : e.outcome}
                  </span>
                  <span className="text-xs text-gray-500 w-16 text-end shrink-0">{t('intake.suggestionsShort', { count: fmt.number(e.suggestions) })}</span>
                  {e.profile_id && (
                    <button className="text-xs text-blue-600 hover:underline shrink-0" onClick={() => router.push(`/profile?id=${encodeURIComponent(e.profile_id!)}`)}>
                      {t('intake.open')}
                    </button>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}

        {accepted && (
          <div className="bg-white rounded-lg border p-4 text-sm">
            <div className="font-medium">{t('intake.batchAccept')}</div>
            <div className="text-gray-500">
              {t('intake.acceptSummary', { accepted: fmt.number(accepted.accepted), skipped: fmt.number(accepted.skipped) })}
              {accepted.skipped > 0 && t('intake.acceptRefused')}
            </div>
            {accepted.fields.length > 0 && <div className="mt-1 text-xs text-gray-500">{t('intake.fieldsList', { fields: accepted.fields.join(', ') })}</div>}
          </div>
        )}
      </div>
    </div>
  );
}
