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
import { AiExtraction, AiPromptPreview, AiSettings, AiSuggestion, FieldDef, Preference, Value } from '@/types/matchmaking';

const formatValue = (v: unknown, t: Translate): string => {
  if (Array.isArray(v)) return (v as unknown[]).map(String).join(t('aiPanel.listSeparator')).replace(/_/g, ' ');
  if (typeof v === 'boolean') return v ? t('profiles.yes') : t('profiles.no');
  return String(v ?? '').replace(/_/g, ' ');
};

/** Reads a profile's free text with the AI model and lists suggestions for the matchmaker to accept or reject. */
export function AiPanel({ profileId, fields, onApplied }: { profileId: string; fields: FieldDef[]; onApplied: () => void }) {
  const { t, fmt, locale } = useI18n();
  const router = useRouter();
  const byKey = useMemo(() => Object.fromEntries(fields.map((f) => [f.key, f])), [fields]);
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [extra, setExtra] = useState('');
  const [suggestions, setSuggestions] = useState<AiSuggestion[]>([]);
  const [result, setResult] = useState<AiExtraction | null>(null);
  const [preview, setPreview] = useState<AiPromptPreview | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const [s, sug] = await Promise.all([invoke<AiSettings>('ai_get_settings', { locale }), invoke<AiSuggestion[]>('ai_pending_suggestions', { profileId })]);
      setSettings(s);
      setSuggestions(sug);
    } catch (e) {
      toast.error(`${e}`);
    }
  }, [profileId, locale]);
  useEffect(() => { void load(); }, [load]);

  // Refresh readiness when a built-in download finishes elsewhere (terminal statuses only).
  useEffect(() => {
    const un = listen<{ status: string }>('builtin-ai-download-progress', (ev) => {
      if (['completed', 'cancelled', 'error'].includes(ev.payload.status)) void load();
    });
    return () => { void un.then((f) => f()); };
  }, [load]);

  const label = (k: string) => byKey[k] ? fieldLabel(byKey[k], locale) : k;

  const read = async () => {
    setBusy(true);
    try {
      const r = await invoke<AiExtraction>('ai_extract_profile', { profileId, extraText: extra.trim() || null, locale });
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
      setPreview(await invoke<AiPromptPreview>('ai_preview_extraction', { profileId, extraText: extra.trim() || null, locale }));
    } catch (e) {
      toast.error(`${e}`);
    }
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
          <span className="flex-1" dir="auto">{settings.not_ready_reason}</span>
          <Button size="sm" variant="outline" onClick={() => router.push('/settings?tab=ai')}>{t('aiPanel.settingsButton')}</Button>
        </div>
      )}

      {settings.ready && (
        <div className="p-4 space-y-3">
          <textarea
            className="w-full border rounded-md p-2 text-sm min-h-[90px]"
            dir="auto"
            placeholder={t('aiPanel.extraPlaceholder')}
            value={extra}
            onChange={(e) => setExtra(e.target.value)}
          />
          <div className="flex flex-wrap gap-2">
            <Button size="sm" disabled={busy} aria-busy={busy} onClick={read}>{busy ? t('aiPanel.reading') : t('aiPanel.read')}</Button>
            <Button size="sm" variant="outline" disabled={busy} onClick={showPreview}>{t('aiPanel.showPreview')}</Button>
          </div>
          {preview && (
            <div className="border rounded-md p-3 bg-gray-50 text-xs space-y-2">
              <div className="flex items-center justify-between gap-2">
                <div className="font-medium">{preview.is_cloud ? t('aiPanel.previewCloud') : t('aiPanel.previewLocal')}</div>
                <Button size="sm" variant="ghost" onClick={() => setPreview(null)}>{t('aiPanel.close')}</Button>
              </div>
              <pre dir="auto" className="whitespace-pre-wrap font-sans max-h-72 overflow-auto">{preview.user}</pre>
            </div>
          )}
        </div>
      )}

      {suggestions.length > 0 && (
        <div className="border-t divide-y">
          <div className="px-4 py-2 text-xs text-amber-800 bg-amber-50">{t('aiPanel.checkWarning')}</div>
          {suggestions.map((s) => {
            const isPref = s.kind === 'preference';
            return (
              <div key={s.id} className="p-4 space-y-2">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-xs px-1.5 py-0.5 rounded bg-gray-100">{isPref ? t('aiPanel.kindPreference') : t('aiPanel.kindFact')}</span>
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
