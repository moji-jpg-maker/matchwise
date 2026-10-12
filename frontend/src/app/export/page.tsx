'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Download, Copy } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { DatasetSummary, ExportDoc } from '@/types/matchmaking';

/// The minimum number of finished matches at which outcome-based learning starts to be worth evaluating.
/// Below it the deterministic baseline is the only ranking, which is what the plan asks for.
const LEARNING_MINIMUM = 100;

export default function ExportPage() {
  const router = useRouter();
  const [summary, setSummary] = useState<DatasetSummary | null>(null);
  const [doc, setDoc] = useState<ExportDoc | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setSummary(await invoke<DatasetSummary>('mm_outcome_dataset_summary'));
    } catch (e) {
      toast.error(`Could not read the decision data: ${e}`);
    }
  }, []);
  useEffect(() => { void load(); }, [load]);

  const build = async () => {
    setBusy(true);
    try {
      const d = await invoke<ExportDoc>('mm_export_outcomes');
      setDoc(d);
      await load();
      toast.success(`${d.row_count} rows, ${d.with_outcome} with an outcome`);
    } catch (e) {
      toast.error(`Could not build the export: ${e}`);
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
    toast.success('Saved. Opening it in a spreadsheet will show any quoted commas correctly.');
  };

  const copy = async () => {
    if (!doc) return;
    await navigator.clipboard.writeText(doc.csv);
    toast.success('Copied to the clipboard');
  };

  const enough = (summary?.finished ?? 0) >= LEARNING_MINIMUM;

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center gap-3">
          <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back">
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h1 className="text-2xl font-semibold">Decision data</h1>
            <p className="text-sm text-gray-500">
              Every tracked pair and how it ended, so ranking can later be checked against what actually happened.
              The file holds no names, no contact details and no notes.
            </p>
          </div>
        </div>

        <div className="bg-white rounded-lg border p-5 space-y-4">
          <h2 className="font-medium">What is here</h2>
          {!summary ? (
            <p className="text-sm text-gray-500">Reading…</p>
          ) : (
            <>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                <Stat label="Tracked pairs" value={summary.matches} />
                <Stat label="Finished" value={summary.finished} />
                <Stat label="With a stored scorecard" value={summary.with_snapshot} />
                <Stat label="Advanced by override" value={summary.overridden} />
              </div>

              {Object.keys(summary.outcome_counts).length > 0 && (
                <div>
                  <div className="text-sm font-medium mb-1">Outcomes</div>
                  <div className="flex flex-wrap gap-2 text-xs">
                    {Object.entries(summary.outcome_counts).map(([k, n]) => (
                      <span key={k} className="px-2 py-1 rounded bg-gray-100">
                        {k.replace(/_/g, ' ')} <span className="text-gray-500">{n}</span>
                      </span>
                    ))}
                  </div>
                </div>
              )}

              {Object.keys(summary.open_by_status).length > 0 && (
                <div>
                  <div className="text-sm font-medium mb-1">Still in progress</div>
                  <div className="flex flex-wrap gap-2 text-xs">
                    {Object.entries(summary.open_by_status).map(([k, n]) => (
                      <span key={k} className="px-2 py-1 rounded bg-blue-50 text-blue-800">
                        {k.replace(/_/g, ' ')} <span className="text-blue-500">{n}</span>
                      </span>
                    ))}
                  </div>
                </div>
              )}

              <div className="text-sm text-gray-500 space-y-1">
                {summary.rule_set_versions.length > 1 && (
                  <p className="text-amber-700">
                    These decisions were made under {summary.rule_set_versions.length} different rule-set versions. A score
                    only means what it meant under the rules in force at the time, which is why each row records its version.
                  </p>
                )}
                {summary.earliest_created_at && (
                  <p>
                    Oldest pair: {new Date(summary.earliest_created_at).toLocaleDateString()}
                    {summary.latest_updated_at ? ` · latest change: ${new Date(summary.latest_updated_at).toLocaleDateString()}` : ''}
                  </p>
                )}
                <p className={enough ? '' : 'text-amber-700'}>
                  {enough
                    ? `${summary.finished} finished matches is enough to start comparing a learned ranking with the deterministic one.`
                    : `${summary.finished} of ${LEARNING_MINIMUM} finished matches. Until there are more, ranking stays fully deterministic — learning from a handful of outcomes would fit noise.`}
                </p>
              </div>
            </>
          )}
        </div>

        <div className="bg-white rounded-lg border p-5 space-y-4">
          <div className="flex items-center justify-between">
            <h2 className="font-medium">Build the file</h2>
            <Button onClick={() => void build()} disabled={busy}>
              {busy ? 'Building…' : 'Build CSV'}
            </Button>
          </div>
          <p className="text-sm text-gray-500">
            One row per pair: the rule-set version and stored scorecard it was judged on, what each person said, the
            outcome, and whether the rules had to be overridden. Each scored dimension becomes its own column; a blank
            means that dimension was never scored for that pair, which is not the same as a zero.
          </p>

          {doc && (
            <>
              <div className="flex flex-wrap items-center gap-2">
                <Button onClick={save}>
                  <Download className="w-4 h-4 mr-2" /> Save as CSV
                </Button>
                <Button variant="outline" onClick={() => void copy()}>
                  <Copy className="w-4 h-4 mr-2" /> Copy
                </Button>
                <span className="text-xs text-gray-500">
                  {doc.row_count} rows · {doc.with_outcome} with an outcome · {doc.columns.length} columns · built{' '}
                  {new Date(doc.generated_at).toLocaleTimeString()}
                </span>
              </div>
              <details className="text-sm">
                <summary className="cursor-pointer text-gray-600">Columns</summary>
                <div className="mt-2 flex flex-wrap gap-1 text-xs">
                  {doc.columns.map((c) => (
                    <code key={c} className="px-1.5 py-0.5 rounded bg-gray-100">{c}</code>
                  ))}
                </div>
              </details>
              <details className="text-sm">
                <summary className="cursor-pointer text-gray-600">Preview (first 20 lines)</summary>
                <pre className="mt-2 p-3 bg-gray-900 text-gray-100 rounded text-xs overflow-x-auto">
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

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded border bg-gray-50 p-3">
      <div className="text-xl font-semibold">{value}</div>
      <div className="text-xs text-gray-500">{label}</div>
    </div>
  );
}

