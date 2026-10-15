'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Download, Copy } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { DatasetSummary, ExportDoc, MatchStatus, Outcome } from '@/types/matchmaking';
import { useI18n } from '@/i18n';
import { OUTCOME_KEY, STATUS_KEY } from '@/lib/workflow';

export default function ExportPage() {
  const router = useRouter();
  const { t, fmt } = useI18n();
  const [summary, setSummary] = useState<DatasetSummary | null>(null);
  const [doc, setDoc] = useState<ExportDoc | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setSummary(await invoke<DatasetSummary>('mm_outcome_dataset_summary'));
      setLoadError(null);
    } catch (e) {
      setLoadError(`${e}`);
      toast.error(t('exportPage.readError', { error: `${e}` }));
    }
  }, [t]);
  useEffect(() => { void load(); }, [load]);

  const build = async () => {
    setBusy(true);
    try {
      const d = await invoke<ExportDoc>('mm_export_outcomes');
      setDoc(d);
      await load();
      toast.success(t('exportPage.built', { rows: fmt.number(d.row_count), withOutcome: fmt.number(d.with_outcome) }));
    } catch (e) {
      toast.error(t('exportPage.buildError', { error: `${e}` }));
    } finally {
      setBusy(false);
    }
  };

  const save = () => {
    if (!doc) return;
    const blob = new Blob([doc.csv], { type: 'text/csv;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    const stamp = new Date().toISOString().slice(0, 10);
    a.href = url;
    a.download = `matchwise-decisions-${stamp}.csv`;
    a.click();
    URL.revokeObjectURL(url);
    toast.success(t('exportPage.saved'));
  };

  const copy = async () => {
    if (!doc) return;
    await navigator.clipboard.writeText(doc.csv);
    toast.success(t('exportPage.copied'));
  };

  // The gate comes from the command (`minimum_finished`), which is the one place it is defined; nothing is hard-coded here.
  const gate = summary?.minimum_finished ?? 0;
  const enough = (summary?.finished ?? 0) >= gate;

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center gap-3">
          <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label={t('profiles.back')}>
            <ArrowLeft className="w-5 h-5 rtl-mirror" />
          </button>
          <div>
            <h1 className="text-2xl font-semibold">{t('exportPage.title')}</h1>
            <p className="text-sm text-gray-500">{t('exportPage.intro')}</p>
          </div>
        </div>

        <div className="bg-white rounded-lg border p-5 space-y-4">
          <h2 className="font-medium">{t('exportPage.whatIsHere')}</h2>
          {!summary ? (
            loadError ? (
              <div className="space-y-2">
                <p className="text-sm text-red-700">{t('exportPage.readError', { error: loadError })}</p>
                <Button variant="outline" size="sm" onClick={() => void load()}>{t('profiles.retry')}</Button>
              </div>
            ) : (
              <p className="text-sm text-gray-500">{t('exportPage.reading')}</p>
            )
          ) : (
            <>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                <Stat label={t('exportPage.trackedPairs')} value={fmt.number(summary.matches)} />
                <Stat label={t('exportPage.finished')} value={fmt.number(summary.finished)} />
                <Stat label={t('exportPage.withSnapshot')} value={fmt.number(summary.with_snapshot)} />
                <Stat label={t('exportPage.overridden')} value={fmt.number(summary.overridden)} />
              </div>

              {Object.keys(summary.outcome_counts).length > 0 && (
                <div>
                  <div className="text-sm font-medium mb-1">{t('exportPage.outcomes')}</div>
                  <div className="flex flex-wrap gap-2 text-xs">
                    {Object.entries(summary.outcome_counts).map(([k, n]) => (
                      <span key={k} className="px-2 py-1 rounded bg-gray-100">
                        {k in OUTCOME_KEY ? t(OUTCOME_KEY[k as Outcome]) : k} <span className="text-gray-500">{fmt.number(n)}</span>
                      </span>
                    ))}
                  </div>
                </div>
              )}

              {Object.keys(summary.open_by_status).length > 0 && (
                <div>
                  <div className="text-sm font-medium mb-1">{t('exportPage.inProgress')}</div>
                  <div className="flex flex-wrap gap-2 text-xs">
                    {Object.entries(summary.open_by_status).map(([k, n]) => (
                      <span key={k} className="px-2 py-1 rounded bg-blue-50 text-blue-800">
                        {k in STATUS_KEY ? t(STATUS_KEY[k as MatchStatus]) : k} <span className="text-blue-500">{fmt.number(n)}</span>
                      </span>
                    ))}
                  </div>
                </div>
              )}

              <div className="text-sm text-gray-500 space-y-1">
                {summary.rule_set_versions.length > 1 && (
                  <p className="text-amber-700">
                    {t('exportPage.multiVersions', { count: fmt.number(summary.rule_set_versions.length) })}
                  </p>
                )}
                {summary.earliest_created_at && (
                  <p>
                    {summary.latest_updated_at
                      ? t('exportPage.oldestLatest', { date: fmt.date(summary.earliest_created_at), latest: fmt.date(summary.latest_updated_at) })
                      : t('exportPage.oldest', { date: fmt.date(summary.earliest_created_at) })}
                  </p>
                )}
                <p className={enough ? '' : 'text-amber-700'}>
                  {enough
                    ? t('exportPage.enough', { finished: fmt.number(summary.finished) })
                    : t('exportPage.notEnough', { finished: fmt.number(summary.finished), gate: fmt.number(gate) })}
                </p>
              </div>
            </>
          )}
        </div>

        <div className="bg-white rounded-lg border p-5 space-y-4">
          <div className="flex items-center justify-between">
            <h2 className="font-medium">{t('exportPage.buildTitle')}</h2>
            <Button onClick={() => void build()} disabled={busy}>
              {busy ? t('exportPage.building') : t('exportPage.buildCsv')}
            </Button>
          </div>
          <p className="text-sm text-gray-500">{t('exportPage.buildHelp')}</p>

          {doc && (
            <>
              <div className="flex flex-wrap items-center gap-2">
                <Button onClick={save}>
                  <Download className="w-4 h-4 me-2" /> {t('exportPage.saveCsv')}
                </Button>
                <Button variant="outline" onClick={() => void copy()}>
                  <Copy className="w-4 h-4 me-2" /> {t('exportPage.copy')}
                </Button>
                <span className="text-xs text-gray-500">
                  {t('exportPage.stats', {
                    rows: fmt.number(doc.row_count),
                    withOutcome: fmt.number(doc.with_outcome),
                    columns: fmt.number(doc.columns.length),
                    time: fmt.dateTime(doc.generated_at),
                  })}
                </span>
              </div>
              <details className="text-sm">
                <summary className="cursor-pointer text-gray-600">{t('exportPage.columns')}</summary>
                <div dir="ltr" className="mt-2 flex flex-wrap gap-1 text-xs">
                  {doc.columns.map((c) => (
                    <code key={c} className="px-1.5 py-0.5 rounded bg-gray-100">{c}</code>
                  ))}
                </div>
              </details>
              <details className="text-sm">
                <summary className="cursor-pointer text-gray-600">{t('exportPage.preview')}</summary>
                <pre dir="ltr" className="mt-2 p-3 bg-gray-900 text-gray-100 rounded text-xs overflow-x-auto">
                  {doc.csv.split('\n').slice(0, 21).join('\n')}
                </pre>
              </details>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded border bg-gray-50 p-3">
      <div className="text-xl font-semibold">{value}</div>
      <div className="text-xs text-gray-500">{label}</div>
    </div>
  );
}
