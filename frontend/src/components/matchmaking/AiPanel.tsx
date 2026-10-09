'use client';

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { describePreference } from '@/lib/conditions';
import { AiExtraction, AiPromptPreview, AiSettings, AiSuggestion, FieldDef, Preference, Value } from '@/types/matchmaking';

const formatValue = (v: unknown): string => {
  if (Array.isArray(v)) return (v as unknown[]).map(String).join(', ').replace(/_/g, ' ');
  if (typeof v === 'boolean') return v ? 'yes' : 'no';
  return String(v ?? '').replace(/_/g, ' ');
};

/** Reads a profile's free text with the AI model and lists suggestions for the matchmaker to accept or reject. */
export function AiPanel({ profileId, fields, onApplied }: { profileId: string; fields: FieldDef[]; onApplied: () => void }) {
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
      const [s, sug] = await Promise.all([invoke<AiSettings>('ai_get_settings'), invoke<AiSuggestion[]>('ai_pending_suggestions', { profileId })]);
      setSettings(s);
      setSuggestions(sug);
    } catch (e) {
      toast.error(`${e}`);
    }
  }, [profileId]);
  useEffect(() => { void load(); }, [load]);

  const label = (k: string) => byKey[k]?.label ?? k;

  const read = async () => {
    setBusy(true);
    try {
      const r = await invoke<AiExtraction>('ai_extract_profile', { profileId, extraText: extra.trim() || null });
      setResult(r);
      setSuggestions(r.suggestions);
      setPreview(null);
      toast.success(r.suggestions.length ? `${r.suggestions.length} suggestion(s) to review` : 'Nothing new found');
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setBusy(false);
    }
  };

  const showPreview = async () => {
    try {
      setPreview(await invoke<AiPromptPreview>('ai_preview_extraction', { profileId, extraText: extra.trim() || null }));
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  const decide = async (s: AiSuggestion, decision: 'accept' | 'accept_verified' | 'reject') => {
    try {
      await invoke('ai_decide_suggestion', { id: s.id, decision });
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
          <h2 className="font-medium">AI profile reading</h2>
          <p className="text-xs text-gray-500">Reads the free text and your intake notes, and suggests fields and preferences. Nothing is changed until you accept it.</p>
        </div>
        {settings.ready && <span className={`text-xs px-2 py-0.5 rounded ${settings.mode?.is_cloud ? 'bg-amber-100 text-amber-800' : 'bg-green-100 text-green-800'}`}>{settings.mode?.is_cloud ? 'cloud' : 'local'}</span>}
      </div>

      {!settings.ready && (
        <div className="p-4 text-sm text-gray-600 flex items-center gap-3">
          <span className="flex-1">{settings.not_ready_reason}</span>
          <Button size="sm" variant="outline" onClick={() => router.push('/ai')}>AI settings</Button>
        </div>
      )}

      {settings.ready && (
        <div className="p-4 space-y-3">
          <textarea
            className="w-full border rounded-md p-2 text-sm min-h-[90px]"
            placeholder="Optional: paste notes from an interview or message. Names, phone numbers and e-mail addresses are removed before anything is read."
            value={extra}
            onChange={(e) => setExtra(e.target.value)}
          />
          <div className="flex flex-wrap gap-2">
            <Button size="sm" disabled={busy} onClick={read}>{busy ? 'Reading…' : 'Read with AI'}</Button>
            <Button size="sm" variant="outline" disabled={busy} onClick={showPreview}>Show exactly what will be sent</Button>
          </div>
          {preview && (
            <div className="border rounded-md p-3 bg-gray-50 text-xs space-y-2">
              <div className="font-medium">{preview.is_cloud ? 'This will leave your computer:' : 'This stays on your computer:'}</div>
              <pre className="whitespace-pre-wrap font-sans max-h-72 overflow-auto">{preview.user}</pre>
            </div>
          )}
        </div>
      )}

      {suggestions.length > 0 && (
        <div className="border-t divide-y">
          <div className="px-4 py-2 text-xs text-amber-800 bg-amber-50">AI-generated. Check each suggestion against what you know before accepting.</div>
          {suggestions.map((s) => {
            const isPref = s.kind === 'preference';
            return (
              <div key={s.id} className="p-4 space-y-2">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-xs px-1.5 py-0.5 rounded bg-gray-100">{isPref ? 'looking for' : 'about them'}</span>
                  <span className="font-medium text-sm">
                    {isPref ? describePreference(s.payload as Preference, byKey) : `${label(s.field)}: ${formatValue(s.payload as Value)}`}
                  </span>
                  {!isPref && <span className="text-xs text-gray-500">confidence {Math.round(s.confidence * 100)}%</span>}
                  {s.conflict && <span className="text-xs px-1.5 py-0.5 rounded bg-red-100 text-red-800">differs from what a person entered</span>}
                </div>
                <blockquote className="text-sm text-gray-600 border-l-2 pl-3 italic">“{s.evidence}”</blockquote>
                <div className="flex flex-wrap gap-2">
                  <Button size="sm" disabled={s.conflict} title={s.conflict ? 'A person already entered a different value' : 'Saved as “suggested by AI”'} onClick={() => decide(s, 'accept')}>Accept</Button>
                  {!isPref && <Button size="sm" variant="outline" title="You have checked it and take responsibility" onClick={() => decide(s, 'accept_verified')}>Accept as verified</Button>}
                  <Button size="sm" variant="ghost" onClick={() => decide(s, 'reject')}>Reject</Button>
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
              <div className="font-medium text-amber-800">Possible contradictions</div>
              <ul className="list-disc pl-5 space-y-1">
                {result.contradictions.map((c, i) => <li key={i}>{c.description}{c.evidence.map((q, j) => <div key={j} className="text-xs text-gray-500 italic">“{q}”</div>)}</li>)}
              </ul>
            </div>
          )}
          {result.missing.length > 0 && (
            <div><span className="font-medium">Not answered in the text: </span>{result.missing.map(label).join(', ')}</div>
          )}
          {result.questions.length > 0 && (
            <div>
              <div className="font-medium">Questions you could ask</div>
              <ul className="list-disc pl-5">{result.questions.map((q, i) => <li key={i}>{q}</li>)}</ul>
            </div>
          )}
          {result.dropped.length > 0 && (
            <details className="text-xs text-gray-500">
              <summary className="cursor-pointer">{result.dropped.length} item(s) discarded because they were invalid or not backed by the text</summary>
              <ul className="list-disc pl-5 mt-1">{result.dropped.map((d, i) => <li key={i}>{d}</li>)}</ul>
            </details>
          )}
        </div>
      )}
    </div>
  );
}
