'use client';

import React, { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, FileUp, ListOrdered, CheckCheck, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { IntakeAcceptReport, IntakeBatchReport } from '@/types/matchmaking';

const CORPUS_PLACEHOLDER = [
  'Paste one person per block, separated by a line of --- (or a blank line).',
  '',
  'I have never smoked and I work as a nurse.',
  '---',
  'Does not smoke. Works as a teacher. Looking for someone calm.',
].join('\n');

const readFile = async (file: File): Promise<{ text: string | null; entries: string[] | null }> => {
  const raw = await file.text();
  if (file.name.toLowerCase().endsWith('.json')) {
    // The file is already split into entries: hand the strings straight to the command so it is not split twice.
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch (e) {
      throw new Error(`The file is not valid JSON: ${e}`);
    }
    if (!Array.isArray(parsed) || parsed.some((x) => typeof x !== 'string')) {
      throw new Error('A JSON corpus must be an array of strings, one per person.');
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
      const r = await readFile(file);
      setEntries(r.entries);
      setCorpus(r.text ?? '');
      setFileName(file.name);
      setReport(null);
      setAccepted(null);
      setBatchId(null);
      toast.success(`Loaded ${file.name}`);
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  const run = async () => {
    if (!corpus.trim() && !entries?.length) {
      toast.error('Paste a corpus, or load a .txt or .json file.');
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
      if (!r.parsed) toast.message('Saved, but nothing was read', { description: r.note ?? undefined });
      else toast.success(`${r.created} created, ${r.suggestions} suggestion(s) to review`);
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
      toast.success(`${r.accepted} accepted, ${r.skipped} left alone`);
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
          <button onClick={() => router.push('/profiles')} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back">
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h1 className="text-2xl font-semibold">Bulk intake</h1>
            <p className="text-sm text-gray-500">
              Turn a pasted corpus into profiles. The raw text is kept beside each profile and is what the AI reads —
              it is never a profile field. The AI only suggests; nothing is accepted until you say so.
            </p>
          </div>
        </div>

        <div className="bg-white rounded-lg border p-4 space-y-2">
          <div className="text-sm font-medium">The person you are working on (optional)</div>
          <p className="text-xs text-gray-500">
            Their own text, in its own box: it becomes its own profile, never one of the entries below. The ranked
            list opens on them.
          </p>
          <Textarea value={target} onChange={(e) => setTarget(e.target.value)} placeholder="Notes or free text about the target person." />
        </div>

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <div className="flex items-center justify-between">
            <div>
              <div className="text-sm font-medium">Corpus</div>
              <p className="text-xs text-gray-500">One person per block, separated by a line of --- (a blank line works if there is no ---).</p>
            </div>
            <label className="flex items-center gap-2 text-sm cursor-pointer">
              <input
                type="file"
                accept=".txt,.json,text/plain,application/json"
                className="hidden"
                onChange={(e) => { const f = e.target.files?.[0]; if (f) void loadFile(f); e.target.value = ''; }}
              />
              <span className="inline-flex items-center gap-1 px-3 py-1.5 border rounded-md hover:bg-gray-50">
                <FileUp className="w-4 h-4" /> {fileName ? 'Replace file' : 'Load .txt / .json'}
              </span>
            </label>
          </div>

          {fileName && <div className="text-xs text-gray-500">Loaded {fileName}{entries ? ` · ${entries.length} entries` : ''}</div>}

          <Textarea
            value={corpus}
            onChange={(e) => { setCorpus(e.target.value); setEntries(null); setFileName(null); }}
            placeholder={CORPUS_PLACEHOLDER}
            className="min-h-[180px] font-mono text-xs"
          />

          <div className="flex flex-wrap gap-2">
            <Button onClick={run} disabled={busy}>{busy ? 'Working…' : 'Create profiles'}</Button>
            {report?.target_profile_id && (
              <Button variant="outline" onClick={() => router.push(`/profile?id=${encodeURIComponent(report.target_profile_id!)}`)}>
                <ListOrdered className="w-4 h-4 mr-1" /> Show ranked list
              </Button>
            )}
            {batchId && report?.parsed && (
              <Button variant="outline" onClick={acceptAll} disabled={busy}>
                <CheckCheck className="w-4 h-4 mr-1" /> Accept all suggestions
              </Button>
            )}
          </div>
        </div>

        {phase !== 'idle' && (
          <div className="bg-white rounded-lg border p-4 space-y-2" role="status" aria-live="polite">
            <div className="flex items-center gap-2 text-sm font-medium">
              <Loader2 className="w-4 h-4 animate-spin" />
              {phase === 'reading'
                ? `Reading ${countEntries(corpus, entries)} entr${countEntries(corpus, entries) === 1 ? 'y' : 'ies'} with the AI…`
                : 'Accepting suggestions…'}
              <span className="text-gray-500 font-normal">{elapsed}s</span>
            </div>
            <div className="h-1.5 bg-gray-100 rounded overflow-hidden">
              <div className="h-full w-1/3 bg-blue-500 rounded animate-pulse" />
            </div>
            <p className="text-xs text-gray-500">
              {phase === 'reading'
                ? 'This can take a while for a long corpus. Stay on this page; the result appears below when it finishes.'
                : 'Applying every suggestion that is safe to accept.'}
            </p>
          </div>
        )}

        {report && (
          <div className="bg-white rounded-lg border">
            <div className="p-4 border-b">
              <div className="font-medium">Result</div>
              <div className="text-sm text-gray-500">
                {report.created} profile(s) created · {report.dropped} dropped · {report.suggestions} suggestion(s)
              </div>
              {!report.parsed && report.note && (
                <div className="mt-1 text-sm text-amber-800 bg-amber-50 rounded p-2">{report.note}</div>
              )}
            </div>
            <div className="divide-y">
              {report.entries.map((e) => (
                <div key={e.index} className="p-3 flex items-start gap-3 text-sm">
                  <span className="text-gray-400 w-8 shrink-0">#{e.index}</span>
                  <div className="flex-1 min-w-0">
                    <div className="font-medium truncate">{e.label}</div>
                    {e.reason && <div className="text-xs text-amber-700">{e.reason}</div>}
                  </div>
                  <span className={`text-xs px-2 py-0.5 rounded shrink-0 ${e.outcome === 'dropped' ? 'bg-red-100 text-red-800' : e.outcome === 'parsed' ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-700'}`}>
                    {e.outcome}
                  </span>
                  <span className="text-xs text-gray-500 w-16 text-right shrink-0">{e.suggestions} sugg.</span>
                  {e.profile_id && (
                    <button className="text-xs text-blue-600 hover:underline shrink-0" onClick={() => router.push(`/profile?id=${encodeURIComponent(e.profile_id!)}`)}>
                      open
                    </button>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}

        {accepted && (
          <div className="bg-white rounded-lg border p-4 text-sm">
            <div className="font-medium">Batch accept</div>
            <div className="text-gray-500">
              {accepted.accepted} accepted · {accepted.skipped} left alone
              {accepted.skipped > 0 && ' (a plain accept is refused where a person already owns a different value)'}
            </div>
            {accepted.fields.length > 0 && <div className="mt-1 text-xs text-gray-500">Fields: {accepted.fields.join(', ')}</div>}
          </div>
        )}
      </div>
    </div>
  );
}
