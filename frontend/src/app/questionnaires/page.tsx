'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Plus, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { DimensionDef, EvidenceLevel, Instrument, InstrumentView } from '@/types/matchmaking';

const LEVEL_STYLE: Record<EvidenceLevel, string> = {
  validated_instrument: 'bg-green-100 text-green-800',
  adapted_instrument: 'bg-amber-100 text-amber-800',
  matchmaker_defined: 'bg-gray-200 text-gray-800',
};

const slug = (s: string, max: number) =>
  s.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '').replace(/^[0-9]+/, '').slice(0, max);

const DEFAULT_LABELS = ['Strongly disagree', 'Disagree', 'Neutral', 'Agree', 'Strongly agree'];
const DEFAULT_LIMITS = 'Written by the matchmaker and not validated. Use it to structure a conversation, not to judge compatibility or to make decisions about a person.';

interface Draft {
  key: string;
  isNew: boolean;
  name: string;
  dimension: string;
  description: string;
  labels: string[];
  subscales: { label: string; method: 'sum' | 'mean' }[];
  items: { text: string; sub: number; reverse: boolean }[];
  level: 'adapted_instrument' | 'matchmaker_defined';
  citation: string;
  limitations: string;
  interpretation: string;
}

const emptyDraft = (dimension: string): Draft => ({
  key: '', isNew: true, name: '', dimension, description: '', labels: [...DEFAULT_LABELS],
  subscales: [{ label: '', method: 'mean' }], items: [{ text: '', sub: 0, reverse: false }, { text: '', sub: 0, reverse: false }, { text: '', sub: 0, reverse: false }],
  level: 'matchmaker_defined', citation: '', limitations: DEFAULT_LIMITS, interpretation: '',
});

function toInstrument(d: Draft): Instrument {
  const key = d.isNew ? slug(d.name, 24) || 'questionnaire' : d.key;
  const subs = d.subscales.map((s, i) => {
    const sk = slug(s.label, 20) || `scale_${i + 1}`;
    return { key: sk, label: s.label.trim(), field: `q_${key}_${sk}`.slice(0, 64), method: s.method };
  });
  return {
    key, name: d.name.trim(), version: 1, kind: 'custom', dimension: d.dimension, description: d.description.trim(),
    evidence: {
      level: d.level,
      sources: d.citation.trim() ? [{ citation: d.citation.trim(), note: 'Source named by the matchmaker.' }] : [],
      limitations: d.limitations.trim(),
      interpretation: d.interpretation.trim(),
    },
    scale: { min: 1, max: d.labels.length, labels: d.labels.map((l) => l.trim()) },
    items: d.items.map((it, i) => ({ id: `q${i + 1}`, text: it.text.trim(), subscale: subs[Math.min(it.sub, subs.length - 1)]?.key ?? '', reverse: it.reverse })),
    subscales: subs,
  };
}

function fromInstrument(i: Instrument): Draft {
  return {
    key: i.key, isNew: false, name: i.name, dimension: i.dimension, description: i.description, labels: [...i.scale.labels],
    subscales: i.subscales.map((s) => ({ label: s.label, method: s.method })),
    items: i.items.map((it) => ({ text: it.text, sub: Math.max(0, i.subscales.findIndex((s) => s.key === it.subscale)), reverse: it.reverse })),
    level: i.evidence.level === 'adapted_instrument' ? 'adapted_instrument' : 'matchmaker_defined',
    citation: i.evidence.sources[0]?.citation ?? '', limitations: i.evidence.limitations, interpretation: i.evidence.interpretation,
  };
}

export default function QuestionnairesPage() {
  const router = useRouter();
  const [list, setList] = useState<InstrumentView[]>([]);
  const [dims, setDims] = useState<DimensionDef[]>([]);
  const [showArchived, setShowArchived] = useState(false);
  const [open, setOpen] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoadError(null);
    try {
      const [l, d] = await Promise.all([invoke<InstrumentView[]>('q_list_instruments', { includeArchived: showArchived }), invoke<DimensionDef[]>('mm_dimension_catalog')]);
      setList(l);
      setDims(d);
    } catch (e) {
      setLoadError(`${e}`);
      toast.error(`${e}`);
    } finally {
      setLoading(false);
    }
  }, [showArchived]);
  useEffect(() => { void load(); }, [load]);

  const save = async () => {
    if (!draft) return;
    setBusy(true);
    try {
      await invoke('q_save_instrument', { instrument: toInstrument(draft) });
      toast.success(draft.isNew ? 'Questionnaire created' : 'Saved as a new version');
      setDraft(null);
      await load();
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setBusy(false);
    }
  };

  const labelOf = (k: string) => dims.find((d) => d.key === k)?.label ?? k;
  const upd = (patch: Partial<Draft>) => setDraft((d) => (d ? { ...d, ...patch } : d));
  const sel = 'border rounded-md px-2 py-1.5 text-sm bg-white';

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back"><ArrowLeft className="w-5 h-5" /></button>
            <div>
              <h1 className="text-2xl font-semibold">Questionnaires and evidence</h1>
              <p className="text-sm text-gray-500">Every questionnaire says where it comes from, how strong the evidence is, and what it cannot tell you.</p>
            </div>
          </div>
          <Button onClick={() => setDraft(emptyDraft(dims[0]?.key ?? 'family'))}><Plus className="w-4 h-4 mr-1" /> New questionnaire</Button>
        </div>

        <div className="bg-blue-50 border border-blue-200 rounded-lg p-4 text-sm text-blue-900 space-y-1">
          <p><b>A caution about matching on personality.</b> Studies of couples find that how similar two people’s trait scores are predicts satisfaction very little, and that questionnaire scores gathered before a relationship explain only a small part of how it turns out. Scores here describe self-reported tendencies. They are not clinical assessments, and nothing in Matchwise diagnoses anyone.</p>
          <p>The rules that use these scores ship switched off. Use them to start conversations; you decide what weight, if any, they get.</p>
        </div>

        <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} /> Show archived</label>

        <div className="space-y-3">
          {loading && <div className="text-sm text-gray-500">Loading…</div>}
          {loadError && !loading && (
            <div className="bg-white rounded-lg border p-6 text-center space-y-2">
              <p className="text-sm text-red-700">Could not load questionnaires: {loadError}</p>
              <Button variant="outline" size="sm" onClick={() => { setLoading(true); void load(); }}>Retry</Button>
            </div>
          )}
          {!loading && !loadError && list.length === 0 && (
            <div className="bg-white rounded-lg border p-6 text-center text-sm text-gray-500">No questionnaires yet.</div>
          )}
          {list.map((v) => {
            const i = v.instrument;
            const id = `${i.key}@${i.version}`;
            return (
              <div key={id} className={`bg-white rounded-lg border ${v.is_latest ? '' : 'opacity-70'}`}>
                <button className="w-full text-left p-4 flex flex-wrap items-center gap-2" onClick={() => setOpen(open === id ? null : id)}>
                  <span className="font-medium">{i.name}</span>
                  <span className="text-xs text-gray-500">v{i.version}{!v.is_latest ? ' (older version)' : ''}</span>
                  <span className={`text-xs px-2 py-0.5 rounded ${LEVEL_STYLE[i.evidence.level]}`}>{v.evidence_label}</span>
                  <span className="text-xs px-2 py-0.5 rounded bg-gray-100">{labelOf(i.dimension)}</span>
                  <span className="text-xs text-gray-500">{i.items.length} questions</span>
                  {v.archived && <span className="text-xs px-2 py-0.5 rounded bg-gray-200">archived</span>}
                </button>
                {open === id && (
                  <div className="border-t p-4 space-y-3 text-sm">
                    {i.description && <p>{i.description}</p>}
                    <div className="bg-amber-50 border border-amber-200 rounded p-3"><div className="font-medium">What this cannot tell you</div><p className="whitespace-pre-line">{i.evidence.limitations}</p></div>
                    {i.evidence.interpretation && <div><div className="font-medium">How to read the scores</div><p>{i.evidence.interpretation}</p></div>}
                    <div>
                      <div className="font-medium">Sources</div>
                      {i.evidence.sources.length === 0 && <p className="text-gray-500">None: this questionnaire has no published evidence behind it.</p>}
                      <ul className="list-disc pl-5 space-y-1">{i.evidence.sources.map((s, k) => <li key={k}>{s.citation}<div className="text-gray-500">{s.note}</div></li>)}</ul>
                    </div>
                    <div>
                      <div className="font-medium">Scores written to the profile</div>
                      <ul className="list-disc pl-5">{i.subscales.map((s) => <li key={s.key}>{s.label} ({s.method === 'sum' ? 'sum' : 'average'} of {i.items.filter((x) => x.subscale === s.key).length} questions) in the field “{s.field}”</li>)}</ul>
                    </div>
                    <details><summary className="cursor-pointer text-gray-600">Show the questions</summary>
                      <ol className="list-decimal pl-5 mt-1">{i.items.map((q) => <li key={q.id}>{q.text}{q.reverse ? ' (reverse scored)' : ''}</li>)}</ol>
                      <div className="text-xs text-gray-500 mt-1">Scale: {i.scale.labels.map((l, n) => `${i.scale.min + n} = ${l}`).join('; ')}</div>
                    </details>
                    <div className="flex gap-2">
                      {i.kind === 'custom' && v.is_latest && <Button size="sm" variant="outline" onClick={() => setDraft(fromInstrument(i))}>Edit (saves a new version)</Button>}
                      {v.is_latest && <Button size="sm" variant="ghost" onClick={async () => { try { await invoke('q_archive_instrument', { key: i.key, archived: !v.archived }); await load(); } catch (e) { toast.error(`${e}`); } }}>{v.archived ? 'Restore' : 'Archive'}</Button>}
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {draft && (
          <div className="bg-white rounded-lg border p-4 space-y-4">
            <h2 className="font-medium">{draft.isNew ? 'New questionnaire' : `Edit ${draft.name}`}</h2>
            <div className="grid grid-cols-3 gap-3 items-center">
              <label className="text-sm font-medium">Name</label>
              <Input className="col-span-2" value={draft.name} onChange={(e) => upd({ name: e.target.value })} />
              <label className="text-sm font-medium">Informs the dimension</label>
              <select className={`${sel} col-span-2`} value={draft.dimension} onChange={(e) => upd({ dimension: e.target.value })}>{dims.map((d) => <option key={d.key} value={d.key}>{d.label}</option>)}</select>
              <label className="text-sm font-medium">Description</label>
              <Input className="col-span-2" placeholder="Shown to the person" value={draft.description} onChange={(e) => upd({ description: e.target.value })} />
            </div>

            <div>
              <div className="text-sm font-medium mb-1">Answer scale</div>
              <div className="flex flex-wrap gap-2 items-center">
                {draft.labels.map((l, n) => <Input key={n} className="w-40" value={l} onChange={(e) => upd({ labels: draft.labels.map((x, j) => (j === n ? e.target.value : x)) })} />)}
                <Button size="sm" variant="ghost" disabled={draft.labels.length >= 7} onClick={() => upd({ labels: [...draft.labels, ''] })}>+ point</Button>
                <Button size="sm" variant="ghost" disabled={draft.labels.length <= 2} onClick={() => upd({ labels: draft.labels.slice(0, -1) })}>− point</Button>
              </div>
            </div>

            <div>
              <div className="flex items-center gap-2 mb-1"><span className="text-sm font-medium">Scores</span><button className="text-sm text-blue-600" onClick={() => upd({ subscales: [...draft.subscales, { label: '', method: 'mean' }] })}>+ add</button></div>
              {draft.subscales.map((s, n) => (
                <div key={n} className="flex items-center gap-2 mb-1">
                  <Input className="w-64" placeholder="What it measures, e.g. Traditional family life" value={s.label} onChange={(e) => upd({ subscales: draft.subscales.map((x, j) => (j === n ? { ...x, label: e.target.value } : x)) })} />
                  <select className={sel} value={s.method} onChange={(e) => upd({ subscales: draft.subscales.map((x, j) => (j === n ? { ...x, method: e.target.value as 'sum' | 'mean' } : x)) })}><option value="mean">average</option><option value="sum">sum</option></select>
                </div>
              ))}
              <p className="text-xs text-gray-500">Each score becomes a numeric profile field (marked sensitive) that rules can use. Reverse-scored questions are flipped before scoring.</p>
            </div>

            <div>
              <div className="flex items-center gap-2 mb-1"><span className="text-sm font-medium">Questions</span><button className="text-sm text-blue-600" onClick={() => upd({ items: [...draft.items, { text: '', sub: 0, reverse: false }] })}>+ add</button></div>
              {draft.items.map((it, n) => (
                <div key={n} className="flex items-center gap-2 mb-1">
                  <span className="w-6 text-sm text-gray-500">{n + 1}</span>
                  <Input className="flex-1" value={it.text} onChange={(e) => upd({ items: draft.items.map((x, j) => (j === n ? { ...x, text: e.target.value } : x)) })} />
                  {draft.subscales.length > 1 && <select className={sel} value={it.sub} onChange={(e) => upd({ items: draft.items.map((x, j) => (j === n ? { ...x, sub: Number(e.target.value) } : x)) })}>{draft.subscales.map((s, k) => <option key={k} value={k}>{s.label || `score ${k + 1}`}</option>)}</select>}
                  <label className="flex items-center gap-1 text-xs"><input type="checkbox" checked={it.reverse} onChange={(e) => upd({ items: draft.items.map((x, j) => (j === n ? { ...x, reverse: e.target.checked } : x)) })} />reverse</label>
                  <button aria-label="Remove question" className="p-1 hover:bg-gray-100 rounded" onClick={() => upd({ items: draft.items.filter((_, j) => j !== n) })}><Trash2 className="w-4 h-4" /></button>
                </div>
              ))}
            </div>

            <div className="space-y-2">
              <div className="text-sm font-medium">Evidence record</div>
              <select className={sel} value={draft.level} onChange={(e) => upd({ level: e.target.value as Draft['level'] })}>
                <option value="matchmaker_defined">Written by me (not validated)</option>
                <option value="adapted_instrument">Adapted from a published questionnaire</option>
              </select>
              {draft.level === 'adapted_instrument' && <Input placeholder="Citation of the original questionnaire" value={draft.citation} onChange={(e) => upd({ citation: e.target.value })} />}
              <label className="text-xs font-medium block">What this questionnaire cannot tell you (required)</label>
              <textarea className="w-full border rounded-md p-2 text-sm min-h-[60px]" value={draft.limitations} onChange={(e) => upd({ limitations: e.target.value })} />
              <label className="text-xs font-medium block">How to read the scores</label>
              <textarea className="w-full border rounded-md p-2 text-sm min-h-[50px]" value={draft.interpretation} onChange={(e) => upd({ interpretation: e.target.value })} />
              <p className="text-xs text-gray-500">Only questionnaires shipped with Matchwise are marked as validated. If you reword or shorten a published one, say so: its evidence no longer carries over.</p>
            </div>

            <div className="flex justify-end gap-2"><Button variant="ghost" onClick={() => setDraft(null)}>Cancel</Button><Button disabled={busy} onClick={save}>Save</Button></div>
          </div>
        )}
      </div>
    </div>
  );
}
