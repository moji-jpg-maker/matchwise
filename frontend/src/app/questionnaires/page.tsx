'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Plus, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useI18n } from '@/i18n';
import type { Translate } from '@/lib/rules';
import { DimensionDef, EvidenceLevel, Instrument, InstrumentView } from '@/types/matchmaking';

const LEVEL_STYLE: Record<EvidenceLevel, string> = {
  validated_instrument: 'bg-green-100 text-green-800',
  adapted_instrument: 'bg-amber-100 text-amber-800',
  matchmaker_defined: 'bg-gray-200 text-gray-800',
};

const slug = (s: string, max: number) =>
  s.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '').replace(/^[0-9]+/, '').slice(0, max);


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

const emptyDraft = (dimension: string, t: Translate): Draft => ({
  key: '', isNew: true, name: '', dimension, description: '',
  labels: [t('questionnaires.defaultLabel1'), t('questionnaires.defaultLabel2'), t('questionnaires.defaultLabel3'), t('questionnaires.defaultLabel4'), t('questionnaires.defaultLabel5')],
  subscales: [{ label: '', method: 'mean' }], items: [{ text: '', sub: 0, reverse: false }, { text: '', sub: 0, reverse: false }, { text: '', sub: 0, reverse: false }],
  level: 'matchmaker_defined', citation: '', limitations: t('questionnaires.defaultLimits'), interpretation: '',
});

function toInstrument(d: Draft, sourceNote: string, fallbackKey: string): Instrument {
  const key = d.isNew ? slug(d.name, 24) || fallbackKey : d.key;
  const subs = d.subscales.map((s, i) => {
    const sk = slug(s.label, 20) || `scale_${i + 1}`;
    return { key: sk, label: s.label.trim(), field: `q_${key}_${sk}`.slice(0, 64), method: s.method };
  });
  return {
    key, name: d.name.trim(), version: 1, kind: 'custom', dimension: d.dimension, description: d.description.trim(),
    evidence: {
      level: d.level,
      sources: d.citation.trim() ? [{ citation: d.citation.trim(), note: sourceNote }] : [],
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
  const { t, fmt, locale } = useI18n();
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
      const [l, d] = await Promise.all([invoke<InstrumentView[]>('q_list_instruments', { includeArchived: showArchived, locale }), invoke<DimensionDef[]>('mm_dimension_catalog', { locale })]);
      setList(l);
      setDims(d);
    } catch (e) {
      setLoadError(`${e}`);
      toast.error(`${e}`);
    } finally {
      setLoading(false);
    }
  }, [showArchived, locale]);
  useEffect(() => { void load(); }, [load]);

  const save = async () => {
    if (!draft) return;
    setBusy(true);
    try {
      await invoke('q_save_instrument', { instrument: toInstrument(draft, t('questionnaires.sourceNote'), `questionnaire_${Date.now().toString(36)}`), locale });
      toast.success(draft.isNew ? t('questionnaires.created') : t('questionnaires.savedVersion'));
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
  const scaleText = (min: number, labels: string[]) =>
    labels.map((label, n) => t('questionnaires.scaleEntry', { value: fmt.number(min + n), label })).join(t('questionnaires.listSeparator'));

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label={t('profiles.back')}><ArrowLeft className="w-5 h-5 rtl-mirror" /></button>
            <div>
              <h1 className="text-2xl font-semibold">{t('questionnaires.title')}</h1>
              <p className="text-sm text-gray-500">{t('questionnaires.intro')}</p>
            </div>
          </div>
          <Button onClick={() => setDraft(emptyDraft(dims[0]?.key ?? 'family', t))}><Plus className="w-4 h-4 me-1" /> {t('questionnaires.newQuestionnaire')}</Button>
        </div>

        <div className="bg-blue-50 border border-blue-200 rounded-lg p-4 text-sm text-blue-900 space-y-1">
          <p><b>{t('questionnaires.cautionLead')}</b> {t('questionnaires.cautionBody')}</p>
          <p>{t('questionnaires.cautionRules')}</p>
        </div>

        <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} /> {t('questionnaires.showArchived')}</label>

        <div className="space-y-3">
          {loading && <div className="text-sm text-gray-500">{t('common.loading')}</div>}
          {loadError && !loading && (
            <div className="bg-white rounded-lg border p-6 text-center space-y-2">
              <p className="text-sm text-red-700">{t('questionnaires.loadError', { error: loadError })}</p>
              <Button variant="outline" size="sm" onClick={() => { setLoading(true); void load(); }}>{t('profiles.retry')}</Button>
            </div>
          )}
          {!loading && !loadError && list.length === 0 && (
            <div className="bg-white rounded-lg border p-6 text-center text-sm text-gray-500">{t('questionnaires.empty')}</div>
          )}
          {list.map((v) => {
            const i = v.instrument;
            const id = `${i.key}@${i.version}`;
            return (
              <div key={id} className={`bg-white rounded-lg border ${v.is_latest ? '' : 'opacity-70'}`}>
                <button className="w-full text-start p-4 flex flex-wrap items-center gap-2" onClick={() => setOpen(open === id ? null : id)}>
                  <span className="font-medium"><bdi>{i.name}</bdi></span>
                  <span className="text-xs text-gray-500">{t('questionnaires.versionTag', { version: fmt.number(i.version) })}{!v.is_latest ? ` ${t('questionnaires.olderVersion')}` : ''}</span>
                  <span className={`text-xs px-2 py-0.5 rounded ${LEVEL_STYLE[i.evidence.level]}`}><bdi dir="auto">{v.evidence_label}</bdi></span>
                  <span className="text-xs px-2 py-0.5 rounded bg-gray-100"><bdi dir="auto">{labelOf(i.dimension)}</bdi></span>
                  <span className="text-xs text-gray-500">{t('questionnaires.questionCount', { count: fmt.number(i.items.length) })}</span>
                  {v.archived && <span className="text-xs px-2 py-0.5 rounded bg-gray-200">{t('questionnaires.archived')}</span>}
                </button>
                {open === id && (
                  <div className="border-t p-4 space-y-3 text-sm">
                    {i.description && <p dir="auto">{i.description}</p>}
                    <div className="bg-amber-50 border border-amber-200 rounded p-3"><div className="font-medium">{t('questionnaires.limitations')}</div><p dir="auto" className="whitespace-pre-line">{i.evidence.limitations}</p></div>
                    {i.evidence.interpretation && <div><div className="font-medium">{t('questionnaires.interpretation')}</div><p dir="auto">{i.evidence.interpretation}</p></div>}
                    <div>
                      <div className="font-medium">{t('questionnaires.sources')}</div>
                      {i.evidence.sources.length === 0 && <p className="text-gray-500">{t('questionnaires.noSources')}</p>}
                      <ul className="list-disc ps-5 space-y-1">{i.evidence.sources.map((s, k) => <li key={k} dir="auto">{s.citation}<div className="text-gray-500">{s.note}</div></li>)}</ul>
                    </div>
                    <div>
                      <div className="font-medium">{t('questionnaires.scoresWritten')}</div>
                      <ul className="list-disc ps-5">{i.subscales.map((s) => <li key={s.key} dir="auto">{t(s.method === 'sum' ? 'questionnaires.scoreSum' : 'questionnaires.scoreMean', { label: s.label, count: fmt.number(i.items.filter((x) => x.subscale === s.key).length), field: s.field })}</li>)}</ul>
                    </div>
                    <details><summary className="cursor-pointer text-gray-600">{t('questionnaires.showQuestions')}</summary>
                      <ol className="list-decimal ps-5 mt-1">{i.items.map((q) => <li key={q.id} dir="auto">{q.text}{q.reverse ? ` ${t('questionnaires.reverseScored')}` : ''}</li>)}</ol>
                      <div className="text-xs text-gray-500 mt-1">{t('questionnaires.scaleLine', { scale: scaleText(i.scale.min, i.scale.labels) })}</div>
                    </details>
                    <div className="flex gap-2">
                      {i.kind === 'custom' && v.is_latest && <Button size="sm" variant="outline" onClick={() => setDraft(fromInstrument(i))}>{t('questionnaires.edit')}</Button>}
                      {v.is_latest && <Button size="sm" variant="ghost" onClick={async () => { try { await invoke('q_archive_instrument', { key: i.key, archived: !v.archived, locale }); await load(); } catch (e) { toast.error(`${e}`); } }}>{v.archived ? t('questionnaires.restore') : t('questionnaires.archive')}</Button>}
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {draft && (
          <div className="bg-white rounded-lg border p-4 space-y-4">
            <h2 className="font-medium">{draft.isNew ? t('questionnaires.newQuestionnaire') : t('questionnaires.editTitle', { name: draft.name })}</h2>
            <div className="grid grid-cols-3 gap-3 items-center">
              <label className="text-sm font-medium">{t('questionnaires.name')}</label>
              <Input dir="auto" className="col-span-2" value={draft.name} onChange={(e) => upd({ name: e.target.value })} />
              <label className="text-sm font-medium">{t('questionnaires.dimension')}</label>
              <select className={`${sel} col-span-2`} value={draft.dimension} onChange={(e) => upd({ dimension: e.target.value })}>{dims.map((d) => <option key={d.key} value={d.key}>{d.label}</option>)}</select>
              <label className="text-sm font-medium">{t('questionnaires.description')}</label>
              <Input dir="auto" className="col-span-2" placeholder={t('questionnaires.descriptionPlaceholder')} value={draft.description} onChange={(e) => upd({ description: e.target.value })} />
            </div>

            <div>
              <div className="text-sm font-medium mb-1">{t('questionnaires.answerScale')}</div>
              <div className="flex flex-wrap gap-2 items-center">
                {draft.labels.map((l, n) => <Input key={n} dir="auto" className="w-40" value={l} onChange={(e) => upd({ labels: draft.labels.map((x, j) => (j === n ? e.target.value : x)) })} />)}
                <Button size="sm" variant="ghost" disabled={draft.labels.length >= 7} onClick={() => upd({ labels: [...draft.labels, ''] })}>{t('questionnaires.addPoint')}</Button>
                <Button size="sm" variant="ghost" disabled={draft.labels.length <= 2} onClick={() => upd({ labels: draft.labels.slice(0, -1) })}>{t('questionnaires.removePoint')}</Button>
              </div>
            </div>

            <div>
              <div className="flex items-center gap-2 mb-1"><span className="text-sm font-medium">{t('questionnaires.scores')}</span><button className="text-sm text-blue-600" onClick={() => upd({ subscales: [...draft.subscales, { label: '', method: 'mean' }] })}>{t('questionnaires.add')}</button></div>
              {draft.subscales.map((s, n) => (
                <div key={n} className="flex items-center gap-2 mb-1">
                  <Input dir="auto" className="w-64" placeholder={t('questionnaires.subscalePlaceholder')} value={s.label} onChange={(e) => upd({ subscales: draft.subscales.map((x, j) => (j === n ? { ...x, label: e.target.value } : x)) })} />
                  <select className={sel} value={s.method} onChange={(e) => upd({ subscales: draft.subscales.map((x, j) => (j === n ? { ...x, method: e.target.value as 'sum' | 'mean' } : x)) })}><option value="mean">{t('questionnaires.average')}</option><option value="sum">{t('questionnaires.sum')}</option></select>
                </div>
              ))}
              <p className="text-xs text-gray-500">{t('questionnaires.scoresNote')}</p>
            </div>

            <div>
              <div className="flex items-center gap-2 mb-1"><span className="text-sm font-medium">{t('questionnaires.questions')}</span><button className="text-sm text-blue-600" onClick={() => upd({ items: [...draft.items, { text: '', sub: 0, reverse: false }] })}>{t('questionnaires.add')}</button></div>
              {draft.items.map((it, n) => (
                <div key={n} className="flex items-center gap-2 mb-1">
                  <span className="w-6 text-sm text-gray-500">{fmt.number(n + 1)}</span>
                  <Input dir="auto" className="flex-1" value={it.text} onChange={(e) => upd({ items: draft.items.map((x, j) => (j === n ? { ...x, text: e.target.value } : x)) })} />
                  {draft.subscales.length > 1 && <select className={sel} value={it.sub} onChange={(e) => upd({ items: draft.items.map((x, j) => (j === n ? { ...x, sub: Number(e.target.value) } : x)) })}>{draft.subscales.map((s, k) => <option key={k} value={k}>{s.label || t('questionnaires.fallbackScore', { number: fmt.number(k + 1) })}</option>)}</select>}
                  <label className="flex items-center gap-1 text-xs"><input type="checkbox" checked={it.reverse} onChange={(e) => upd({ items: draft.items.map((x, j) => (j === n ? { ...x, reverse: e.target.checked } : x)) })} />{t('questionnaires.reverse')}</label>
                  <button aria-label={t('questionnaires.removeQuestion')} className="p-1 hover:bg-gray-100 rounded" onClick={() => upd({ items: draft.items.filter((_, j) => j !== n) })}><Trash2 className="w-4 h-4" /></button>
                </div>
              ))}
            </div>

            <div className="space-y-2">
              <div className="text-sm font-medium">{t('questionnaires.evidenceRecord')}</div>
              <select className={sel} value={draft.level} onChange={(e) => upd({ level: e.target.value as Draft['level'] })}>
                <option value="matchmaker_defined">{t('questionnaires.levelDefined')}</option>
                <option value="adapted_instrument">{t('questionnaires.levelAdapted')}</option>
              </select>
              {draft.level === 'adapted_instrument' && <Input dir="auto" placeholder={t('questionnaires.citationPlaceholder')} value={draft.citation} onChange={(e) => upd({ citation: e.target.value })} />}
              <label className="text-xs font-medium block">{t('questionnaires.limitationsRequired')}</label>
              <textarea dir="auto" className="w-full border rounded-md p-2 text-sm min-h-[60px]" value={draft.limitations} onChange={(e) => upd({ limitations: e.target.value })} />
              <label className="text-xs font-medium block">{t('questionnaires.interpretation')}</label>
              <textarea dir="auto" className="w-full border rounded-md p-2 text-sm min-h-[50px]" value={draft.interpretation} onChange={(e) => upd({ interpretation: e.target.value })} />
              <p className="text-xs text-gray-500">{t('questionnaires.validatedNote')}</p>
            </div>

            <div className="flex justify-end gap-2"><Button variant="ghost" onClick={() => setDraft(null)}>{t('common.cancel')}</Button><Button disabled={busy} onClick={save}>{t('common.save')}</Button></div>
          </div>
        )}
      </div>
    </div>
  );
}
