'use client';

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { AiClaim, AiPairView, AiPromptPreview, AiSettings } from '@/types/matchmaking';

const SECTIONS: { key: keyof AiPairView['analysis']; title: string; tone: string }[] = [
  { key: 'why_it_may_work', title: 'Why they may work', tone: 'text-green-800' },
  { key: 'potential_challenges', title: 'Potential challenges', tone: 'text-amber-800' },
  { key: 'important_differences', title: 'Important differences', tone: 'text-gray-800' },
  { key: 'questions_to_discuss', title: 'Questions worth discussing', tone: 'text-blue-800' },
  { key: 'missing_information', title: 'Missing information', tone: 'text-gray-800' },
];

/** An AI explanation of a tracked match. It cites the facts it relies on and cannot change the score or status. */
export function MatchAiPanel({ matchId, nameA, nameB }: { matchId: string; nameA: string; nameB: string }) {
  const router = useRouter();
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [view, setView] = useState<AiPairView | null>(null);
  const [preview, setPreview] = useState<AiPromptPreview | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const [s, v] = await Promise.all([invoke<AiSettings>('ai_get_settings'), invoke<AiPairView | null>('ai_get_match_analysis', { matchId })]);
      setSettings(s);
      setView(v);
    } catch (e) {
      toast.error(`${e}`);
    }
  }, [matchId]);
  useEffect(() => { void load(); }, [load]);

  const facts = useMemo(() => Object.fromEntries((view?.facts ?? []).map((f) => [f.id, f.text])), [view]);
  const who = (id: string) => (id.startsWith('A') ? nameA : id.startsWith('B') ? nameB : id.startsWith('P') ? 'Preference' : 'Rules');

  const generate = async () => {
    setBusy(true);
    try {
      setView(await invoke<AiPairView>('ai_analyse_match', { matchId }));
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
          {c.text}{' '}
          {c.refs.map((r) => (
            <span key={r} title={`${who(r)}: ${facts[r] ?? ''}`} className="inline-block text-[10px] px-1 py-0.5 mr-1 rounded bg-gray-100 text-gray-600 cursor-help align-middle">{r}</span>
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
          <h2 className="font-medium">AI assessment</h2>
          <p className="text-xs text-gray-500">An explanation written from the facts and the rule results. It cannot change the score, eligibility or status.</p>
        </div>
        {settings.ready ? (
          <div className="flex gap-2">
            <Button size="sm" variant="outline" disabled={busy} onClick={async () => { try { setPreview(await invoke<AiPromptPreview>('ai_preview_match', { matchId })); } catch (e) { toast.error(`${e}`); } }}>What will be sent</Button>
            <Button size="sm" disabled={busy} onClick={generate}>{busy ? 'Writing…' : view ? 'Write again' : 'Explain this match'}</Button>
          </div>
        ) : (
          <Button size="sm" variant="outline" onClick={() => router.push('/ai')}>AI settings</Button>
        )}
      </div>
      {!settings.ready && !view && <p className="p-4 text-sm text-gray-600">{settings.not_ready_reason}</p>}
      {preview && (
        <div className="p-4 border-b bg-gray-50 text-xs space-y-2">
          <div className="font-medium">{preview.is_cloud ? 'This will leave your computer (names are replaced by A and B):' : 'This stays on your computer:'}</div>
          <pre className="whitespace-pre-wrap font-sans max-h-72 overflow-auto">{preview.user}</pre>
        </div>
      )}
      {view && a && (
        <div className="p-4 space-y-4 text-sm">
          <div className="flex flex-wrap items-center gap-2 text-xs text-gray-500">
            <span>Written {new Date(view.created_at).toLocaleString()} by {view.model} ({view.is_cloud ? 'cloud' : 'local'})</span>
            {view.stale && <span className="px-2 py-0.5 rounded bg-amber-100 text-amber-800">out of date: profiles, preferences or rules changed since</span>}
          </div>
          {a.overall_assessment && (
            <div className="bg-gray-50 border rounded p-3"><div className="font-medium mb-1">Overall</div><Claims items={[a.overall_assessment]} /></div>
          )}
          {SECTIONS.map(({ key, title, tone }) => {
            const items = a[key] as AiClaim[];
            return items.length > 0 ? (
              <div key={key}>
                <div className={`font-medium mb-1 ${tone}`}>{title}</div>
                <Claims items={items} />
              </div>
            ) : null;
          })}
          <p className="text-xs text-gray-500">Hover a tag such as A2 to see the fact behind a statement. AI-generated: verify before relying on it.</p>
          {a.dropped.length > 0 && (
            <details className="text-xs text-gray-500">
              <summary className="cursor-pointer">{a.dropped.length} statement(s) removed because they cited nothing from the facts</summary>
              <ul className="list-disc pl-5 mt-1">{a.dropped.map((d, i) => <li key={i}>{d}</li>)}</ul>
            </details>
          )}
        </div>
      )}
    </div>
  );
}
