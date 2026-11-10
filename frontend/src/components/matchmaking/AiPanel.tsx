'use client';

import { fieldLabel } from '@/lib/field-label';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { describePreference } from '@/lib/conditions';
import { useI18n } from '@/i18n';
import type { Translate } from '@/lib/rules';
import { AiExtraction, AiPromptPreview, AiSettings, AiSuggestion, ExtractionPresetsView, FieldDef, Preference, Value } from '@/types/matchmaking';

const formatValue = (v: unknown, t: Translate): string => {
  if (Array.isArray(v)) return (v as unknown[]).map(String).join(t('aiPanel.listSeparator')).replace(/_/g, ' ');
  if (typeof v === 'boolean') return v ? t('profiles.yes') : t('profiles.no');
  return String(v ?? '').replace(/_/g, ' ');
};

/** "·"-joined line of what each tier did, so a partial run never looks like a silent failure. */
const seconds = (ms: number): string => (ms / 1000).toFixed(ms < 10000 ? 1 : 0);

/** Reads a profile's free text with the AI model and lists suggestions for the matchmaker to accept or reject. */
export function AiPanel({ profileId, fields, onApplied }: { profileId: string; fields: FieldDef[]; onApplied: () => void }) {
  const { t, fmt, locale } = useI18n();
  const router = useRouter();
  const byKey = useMemo(() => Object.fromEntries(fields.map((f) => [f.key, f])), [fields]);
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [presets, setPresets] = useState<ExtractionPresetsView | null>(null);
  const [presetId, setPresetId] = useState('');
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [withPreferences, setWithPreferences] = useState(false);
  const [extra, setExtra] = useState('');
  const [suggestions, setSuggestions] = useState<AiSuggestion[]>([]);
  const [result, setResult] = useState<AiExtraction | null>(null);
  const [preview, setPreview] = useState<AiPromptPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [seeded, setSeeded] = useState(false);

  const choosePreset = useCallback((view: ExtractionPresetsView, id: string) => {
    setPresetId(id);
    const preset = view.presets.find((p) => p.id === id);
    if (preset) {
      setSelected(new Set(preset.fields));
      setWithPreferences(preset.with_preferences);
    } else {
      setSelected(new Set(view.fields.filter((f) => !f.sensitive).map((f) => f.key)));
      setWithPreferences(false);
    }
  }, []);

  const load = useCallback(async () => {
    try {
      const [s, sug, view] = await Promise.all([
        invoke<AiSettings>('ai_get_settings', { locale }),
        invoke<AiSuggestion[]>('ai_pending_suggestions', { profileId }),
        invoke<ExtractionPresetsView>('mm_extraction_presets', { locale }),
      ]);
      setSettings(s);
      setSuggestions(sug);
      setPresets(view);
      // Default the run to the saved default preset, but never undo a choice already made on this screen.
      if (!seeded) {
        choosePreset(view, view.presets.find((p) => p.is_default)?.id ?? '');
        setSeeded(true);
      }
    } catch (e) {
      toast.error(`${e}`);
    }
  }, [profileId, locale, choosePreset, seeded]);
  useEffect(() => { void load(); }, [load]);

  // Refresh readiness when a built-in download finishes elsewhere (terminal statuses only).
  useEffect(() => {
    const un = listen<{ status: string }>('builtin-ai-download-progress', (ev) => {
      if (['completed', 'cancelled', 'error'].includes(ev.payload.status)) void load();
    });
    return () => { void un.then((f) => f()); };
  }, [load]);

  const label = (k: string) => byKey[k] ? fieldLabel(byKey[k], locale) : k;

  const chosenFields = () => (presets && selected.size > 0 ? [...selected] : null);

  const read = async () => {
    setBusy(true);
    try {
      const r = await invoke<AiExtraction>('ai_extract_profile', { profileId, extraText: extra.trim() || null, locale, fields: chosenFields(), withPreferences });
      setResult(r);
      setSuggestions(r.suggestions);
      setPreview(null);
      toast.success(r.suggestions.length === 0 ? t('aiPanel.nothingNew') : r.suggestions.length === 1 ? t('aiPanel.foundOne') : t('aiPanel.foundMany', { count: fmt.number(r.suggestions.length) }));
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setBusy(false);
    }
  };

  const showPreview = async () => {
    try {
      setPreview(await invoke<AiPromptPreview>('ai_preview_extraction', { profileId, extraText: extra.trim() || null, locale, fields: chosenFields(), withPreferences }));
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  const fieldList = presets?.fields ?? [];
  const toggleField = (key: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key); else next.add(key);
      return next;
    });
    setPresetId('');
  };

  const rulesLine = (r: AiExtraction) => {
    const ms = fmt.number(r.timings.rules_ms);
    if (r.rule_count === 0) return t('aiPanel.rulesNone', { ms });
    return r.rule_count === 1 ? t('aiPanel.rulesOne', { ms }) : t('aiPanel.rulesMany', { count: fmt.number(r.rule_count), ms });
  };

  const statusLine = (r: AiExtraction) => {
    const parts = [rulesLine(r)];
    if (r.llm_status === 'ok') parts.push(r.ai_count === 1 ? t('aiPanel.aiReadOne', { seconds: seconds(r.timings.llm_ms) }) : t('aiPanel.aiReadMany', { count: fmt.number(r.ai_count), seconds: seconds(r.timings.llm_ms) }));
    if (r.llm_status === 'skipped_nothing_left') parts.push(t('aiPanel.aiNothingLeft'));
    if (r.llm_status === 'unavailable') parts.push(t('aiPanel.aiUnavailable', { reason: r.llm_error ?? '' }));
    if (r.llm_status === 'failed') parts.push(t('aiPanel.aiFailed', { reason: r.llm_error ?? '' }));
    return parts.join(t('aiPanel.statusSeparator'));
  };

  const decide = async (s: AiSuggestion, decision: 'accept' | 'accept_verified' | 'reject') => {
    try {
      await invoke('ai_decide_suggestion', { id: s.id, decision, locale });
      setSuggestions((cur) => cur.filter((x) => x.id !== s.id));
      if (decision !== 'reject') onApplied();
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  if (!settings) return null;

  return (
    <div className="bg-white rounded-lg border">
      <div className="p-4 border-b flex items-center gap-3">
        <div className="flex-1">
          <h2 className="font-medium">{t('aiPanel.title')}</h2>
          <p className="text-xs text-gray-500">{t('aiPanel.intro')}</p>
        </div>
        {settings.ready && <span className={`text-xs px-2 py-0.5 rounded ${settings.mode?.is_cloud ? 'bg-amber-100 text-amber-800' : 'bg-green-100 text-green-800'}`}>{settings.mode?.is_cloud ? t('ai.logCloud') : t('ai.logLocal')}</span>}
      </div>

      {!settings.ready && (
        <div className="p-4 text-sm text-gray-600 flex items-center gap-3">
          <span className="flex-1" dir="auto">{settings.not_ready_reason ?? t('aiPanel.rulesOnly')}</span>
          <Button size="sm" variant="outline" onClick={() => router.push('/settings?tab=ai')}>{t('aiPanel.settingsButton')}</Button>
        </div>
      )}

      <div className="p-4 space-y-3">
        <textarea
          className="w-full border rounded-md p-2 text-sm min-h-[90px]"
          dir="auto"
          placeholder={t('aiPanel.extraPlaceholder')}
          value={extra}
          onChange={(e) => setExtra(e.target.value)}
        />
        <div className="flex flex-wrap items-end gap-2 text-sm">
          <label className="space-y-1">
            <span className="block text-xs text-gray-500">{t('aiPanel.presetLabel')}</span>
            <select
              className="border rounded-md px-2 py-1 text-sm min-w-40"
              value={presetId}
              onChange={(e) => { if (presets) choosePreset(presets, e.target.value); }}
            >
              <option value="">{t('aiPanel.presetCustom')}</option>
              {presets?.presets.map((p) => <option key={p.id} value={p.id}>{p.name}{p.is_default ? t('aiPanel.presetDefaultTag') : ''}</option>)}
            </select>
          </label>
          <details className="flex-1 min-w-48">
            <summary className="cursor-pointer text-xs text-gray-600">{t('aiPanel.fieldsSummary', { chosen: fmt.number(selected.size), total: fmt.number(fieldList.length) })}</summary>
            <div className="mt-2 max-h-48 overflow-auto grid grid-cols-1 sm:grid-cols-2 gap-1">
              {fieldList.map((f) => (
                <label key={f.key} className="flex items-center gap-2 text-xs">
                  <input type="checkbox" checked={selected.has(f.key)} onChange={() => toggleField(f.key)} />
                  <bdi dir="auto">{f.label}</bdi>
                </label>
              ))}
            </div>
          </details>
        </div>
        <label className="flex items-start gap-2 text-xs text-gray-600">
          <input type="checkbox" className="mt-0.5" checked={withPreferences} onChange={(e) => setWithPreferences(e.target.checked)} />
          <span>{t('aiPanel.withPreferences')}</span>
        </label>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" disabled={busy} aria-busy={busy} onClick={read}>{busy ? t('aiPanel.reading') : settings.ready ? t('aiPanel.read') : t('aiPanel.readRules')}</Button>
          <Button size="sm" variant="outline" disabled={busy || !settings.ready} onClick={showPreview}>{t('aiPanel.showPreview')}</Button>
        </div>
        {result && <p className="text-xs text-gray-600" dir="auto">{statusLine(result)}</p>}
        {preview && (
          <div className="border rounded-md p-3 bg-gray-50 text-xs space-y-2">
            <div className="flex items-center justify-between gap-2">
              <div className="font-medium" dir="auto">{preview.will_send ? (preview.is_cloud ? t('aiPanel.previewCloud') : t('aiPanel.previewLocal')) : t('aiPanel.previewNothing')}</div>
              <Button size="sm" variant="ghost" onClick={() => setPreview(null)}>{t('aiPanel.close')}</Button>
            </div>
            <pre dir="auto" className="whitespace-pre-wrap font-sans max-h-72 overflow-auto">{preview.user}</pre>
          </div>
        )}
      </div>

      {suggestions.length > 0 && (
        <div className="border-t divide-y">
          <div className="px-4 py-2 text-xs text-amber-800 bg-amber-50">{t('aiPanel.checkWarning')}</div>
          {suggestions.map((s) => {
            const isPref = s.kind === 'preference';
            return (
              <div key={s.id} className="p-4 space-y-2">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-xs px-1.5 py-0.5 rounded bg-gray-100">{isPref ? t('aiPanel.kindPreference') : t('aiPanel.kindFact')}</span>
                  <span className={`text-xs px-1.5 py-0.5 rounded ${s.source === 'rule' ? 'bg-sky-100 text-sky-800' : 'bg-violet-100 text-violet-800'}`}>{s.source === 'rule' ? t('aiPanel.sourceRule') : t('aiPanel.sourceAi')}</span>
                  <span className="font-medium text-sm">
                    {isPref ? describePreference(s.payload as Preference, byKey, t, locale) : `${label(s.field)}: ${formatValue(s.payload as Value, t)}`}
                  </span>
                  {!isPref && <span className="text-xs text-gray-500">{t('aiPanel.confidence', { percent: fmt.percent(s.confidence) })}</span>}
                  {s.conflict && <span className="text-xs px-1.5 py-0.5 rounded bg-red-100 text-red-800">{t('aiPanel.conflict')}</span>}
                </div>
                <blockquote dir="auto" className="text-sm text-gray-600 border-s-2 ps-3 italic">“{s.evidence}”</blockquote>
                <div className="flex flex-wrap gap-2">
                  <Button size="sm" disabled={s.conflict} title={s.conflict ? t('aiPanel.conflictTitle') : t('aiPanel.acceptTitle')} onClick={() => decide(s, 'accept')}>{t('aiPanel.accept')}</Button>
                  {!isPref && <Button size="sm" variant="outline" title={t('aiPanel.acceptVerifiedTitle')} onClick={() => decide(s, 'accept_verified')}>{t('aiPanel.acceptVerified')}</Button>}
                  <Button size="sm" variant="ghost" onClick={() => decide(s, 'reject')}>{t('aiPanel.reject')}</Button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {result && (result.contradictions.length > 0 || result.missing.length > 0 || result.questions.length > 0 || result.dropped.length > 0) && (
        <div className="border-t p-4 space-y-3 text-sm">
          {result.contradictions.length > 0 && (
            <div>
              <div className="font-medium text-amber-800">{t('aiPanel.contradictions')}</div>
              <ul className="list-disc ps-5 space-y-1">
                {result.contradictions.map((c, i) => <li key={i} dir="auto">{c.description}{c.evidence.map((q, j) => <div key={j} className="text-xs text-gray-500 italic">“{q}”</div>)}</li>)}
              </ul>
            </div>
          )}
          {result.missing.length > 0 && (
            <div><span className="font-medium">{t('aiPanel.missing')}</span>{result.missing.map(label).join(t('aiPanel.listSeparator'))}</div>
          )}
          {result.questions.length > 0 && (
            <div>
              <div className="font-medium">{t('aiPanel.questions')}</div>
              <ul className="list-disc ps-5">{result.questions.map((q, i) => <li key={i} dir="auto">{q}</li>)}</ul>
            </div>
          )}
          {result.dropped.length > 0 && (
            <details className="text-xs text-gray-500">
              <summary className="cursor-pointer">{result.dropped.length === 1 ? t('aiPanel.droppedOne') : t('aiPanel.droppedMany', { count: fmt.number(result.dropped.length) })}</summary>
              <ul className="list-disc ps-5 mt-1">{result.dropped.map((d, i) => <li key={i} dir="auto">{d}</li>)}</ul>
            </details>
          )}
        </div>
      )}
    </div>
  );
}
