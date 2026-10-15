'use client';

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { AiClaim, AiPairView, AiPromptPreview, AiSettings } from '@/types/matchmaking';
import { useI18n } from '@/i18n';
import type { MessageKey } from '@/i18n/types';

const SECTIONS: { key: keyof AiPairView['analysis']; title: MessageKey; tone: string }[] = [
  { key: 'why_it_may_work', title: 'matchAi.sWhy', tone: 'text-green-800' },
  { key: 'potential_challenges', title: 'matchAi.sChallenges', tone: 'text-amber-800' },
  { key: 'important_differences', title: 'matchAi.sDifferences', tone: 'text-gray-800' },
  { key: 'questions_to_discuss', title: 'matchAi.sQuestions', tone: 'text-blue-800' },
  { key: 'missing_information', title: 'matchAi.sMissing', tone: 'text-gray-800' },
];

/** An AI explanation of a tracked match. It cites the facts it relies on and cannot change the score or status. */
export function MatchAiPanel({ matchId, nameA, nameB }: { matchId: string; nameA: string; nameB: string }) {
  const { t, fmt, locale } = useI18n();
  const router = useRouter();
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [view, setView] = useState<AiPairView | null>(null);
  const [preview, setPreview] = useState<AiPromptPreview | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const [s, v] = await Promise.all([invoke<AiSettings>('ai_get_settings', { locale }), invoke<AiPairView | null>('ai_get_match_analysis', { matchId })]);
      setSettings(s);
      setView(v);
    } catch (e) {
      toast.error(`${e}`);
    }
  }, [matchId, locale]);
  useEffect(() => { void load(); }, [load]);

  // Refresh readiness when a built-in download finishes elsewhere (terminal statuses only).
  useEffect(() => {
    const un = listen<{ status: string }>('builtin-ai-download-progress', (ev) => {
      if (['completed', 'cancelled', 'error'].includes(ev.payload.status)) void load();
    });
    return () => { void un.then((f) => f()); };
  }, [load]);

  const facts = useMemo(() => Object.fromEntries((view?.facts ?? []).map((f) => [f.id, f.text])), [view]);
  const who = (id: string) => (id.startsWith('A') ? nameA : id.startsWith('B') ? nameB : id.startsWith('P') ? t('matchAi.prefLabel') : t('matchAi.rulesLabel'));

  const generate = async () => {
    setBusy(true);
    try {
      setView(await invoke<AiPairView>('ai_analyse_match', { matchId, locale }));
      setPreview(null);
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setBusy(false);
    }
  };

  const Claims = ({ items }: { items: AiClaim[] }) => (
    <ul className="space-y-1.5">
      {items.map((c, i) => (
        <li key={i}>
          <span dir="auto">{c.text}</span>{' '}
          {c.refs.map((r) => (
            <span key={r} title={`${who(r)}: ${facts[r] ?? ''}`} dir="ltr" className="inline-block text-[10px] px-1 py-0.5 me-1 rounded bg-gray-100 text-gray-600 cursor-help align-middle">{r}</span>
          ))}
        </li>
      ))}
    </ul>
  );

  if (!settings) return null;
  const a = view?.analysis;

  return (
    <div className="bg-white rounded-lg border">
      <div className="p-4 border-b flex items-center gap-3">
        <div className="flex-1">
          <h2 className="font-medium">{t('matchAi.title')}</h2>
          <p className="text-xs text-gray-500">{t('matchAi.intro')}</p>
        </div>
        {settings.ready ? (
          <div className="flex gap-2">
            <Button size="sm" variant="outline" disabled={busy} onClick={async () => { try { setPreview(await invoke<AiPromptPreview>('ai_preview_match', { matchId, locale })); } catch (e) { toast.error(`${e}`); } }}>{t('matchAi.whatSent')}</Button>
            <Button size="sm" disabled={busy} aria-busy={busy} onClick={generate}>{busy ? t('matchAi.writing') : view ? t('matchAi.writeAgain') : t('matchAi.explain')}</Button>
          </div>
        ) : (
          <Button size="sm" variant="outline" onClick={() => router.push('/settings?tab=ai')}>{t('matchAi.settings')}</Button>
        )}
      </div>
      {!settings.ready && !view && <p className="p-4 text-sm text-gray-600">{settings.not_ready_reason}</p>}
      {preview && (
        <div className="p-4 border-b bg-gray-50 text-xs space-y-2">
          <div className="font-medium">{preview.is_cloud ? t('matchAi.leaves') : t('matchAi.stays')}</div>
          <pre dir="auto" className="whitespace-pre-wrap font-sans max-h-72 overflow-auto">{preview.user}</pre>
        </div>
      )}
      {view && a && (
        <div className="p-4 space-y-4 text-sm">
          <div className="flex flex-wrap items-center gap-2 text-xs text-gray-500">
            <span>{t(view.is_cloud ? 'matchAi.writtenCloud' : 'matchAi.writtenLocal', { date: fmt.dateTime(view.created_at), model: view.model })}</span>
            {view.stale && <span className="px-2 py-0.5 rounded bg-amber-100 text-amber-800">{t('matchAi.stale')}</span>}
          </div>
          {a.overall_assessment && (
            <div className="bg-gray-50 border rounded p-3"><div className="font-medium mb-1">{t('matchAi.overall')}</div><Claims items={[a.overall_assessment]} /></div>
          )}
          {SECTIONS.map(({ key, title, tone }) => {
            const items = a[key] as AiClaim[];
            return items.length > 0 ? (
              <div key={key}>
                <div className={`font-medium mb-1 ${tone}`}>{t(title)}</div>
                <Claims items={items} />
              </div>
            ) : null;
          })}
          <p className="text-xs text-gray-500">{t('matchAi.hover')}</p>
          {a.dropped.length > 0 && (
            <details className="text-xs text-gray-500">
              <summary className="cursor-pointer">{t('matchAi.dropped', { count: fmt.number(a.dropped.length) })}</summary>
              <ul className="list-disc ps-5 mt-1">{a.dropped.map((d, i) => <li key={i} dir="auto">{d}</li>)}</ul>
            </details>
          )}
        </div>
      )}
    </div>
  );
}
