'use client';

import { fieldLabel } from '@/lib/field-label';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useI18n } from '@/i18n';
import { FieldDef, TelegramInbox, TelegramStatus, kindName } from '@/types/matchmaking';

export default function TelegramSettingsPage() {
  const { t, fmt, locale } = useI18n();
  const router = useRouter();
  const [status, setStatus] = useState<TelegramStatus | null>(null);
  const [token, setToken] = useState('');
  const [fields, setFields] = useState<FieldDef[]>([]);
  const [shared, setShared] = useState<Set<string>>(new Set());
  const [firstName, setFirstName] = useState(true);
  const [inbox, setInbox] = useState<TelegramInbox | null>(null);
  const [busy, setBusy] = useState(false);

  const apply = useCallback((s: TelegramStatus) => {
    setStatus(s);
    setShared(new Set(s.introduction_fields));
    setFirstName(s.show_first_name);
  }, []);

  const load = useCallback(async () => {
    try {
      const [s, f, ib] = await Promise.all([
        invoke<TelegramStatus>('tg_get_status'),
        invoke<FieldDef[]>('mm_list_fields'),
        invoke<TelegramInbox>('tg_inbox'),
      ]);
      apply(s);
      setFields(f.filter((d) => kindName(d.kind) !== 'records' && !s.never_shared.includes(d.key)));
      setInbox(ib);
    } catch (e) {
      toast.error(`${e}`);
    }
  }, [apply]);

  useEffect(() => { void load(); }, [load]);

  // keep the running/last-error indicators fresh while the page is open
  const [pollFailures, setPollFailures] = useState(0);
  useEffect(() => {
    const timer = setInterval(async () => {
      try {
        setStatus(await invoke<TelegramStatus>('tg_get_status'));
        setPollFailures(0);
      } catch {
        setPollFailures((n) => n + 1);
      }
    }, 5000);
    return () => clearInterval(timer);
  }, []);

  const act = async (fn: () => Promise<TelegramStatus>, ok?: string) => {
    setBusy(true);
    try {
      const s = await fn();
      setStatus(s);
      if (ok) toast.success(ok);
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setBusy(false);
    }
  };

  const saveSharing = () =>
    act(async () => {
      const s = await invoke<TelegramStatus>('tg_set_sharing', { fields: Array.from(shared), showFirstName: firstName });
      apply(s);
      return s;
    }, t('telegram.sharingSaved'));

  const sharedDirty = useMemo(
    () => status != null && (firstName !== status.show_first_name || shared.size !== status.introduction_fields.length || status.introduction_fields.some((k) => !shared.has(k))),
    [firstName, shared, status]
  );

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-3xl mx-auto p-6 space-y-5">
        <div className="flex items-center gap-3">
          <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label={t('profiles.back')}><ArrowLeft className="w-5 h-5 rtl-mirror" /></button>
          <div>
            <h1 className="text-2xl font-semibold">{t('telegram.title')}</h1>
            <p className="text-sm text-gray-500">{t('telegram.intro')}</p>
          </div>
        </div>

        {pollFailures >= 3 && (
          <div className="bg-red-50 border border-red-200 rounded p-2 text-sm text-red-800">
            {t('telegram.pollFailed', { count: fmt.number(pollFailures) })}
          </div>
        )}

        <div className="bg-amber-50 border border-amber-200 rounded-lg p-4 text-sm space-y-1">
          <div className="font-medium">{t('telegram.privacyTitle')}</div>
          <p>{t('telegram.privacyLocal')}</p>
          <p>{t('telegram.privacyBot')}</p>
        </div>

        {status && (
          <div className="bg-white rounded-lg border p-4 space-y-3">
            <h2 className="font-medium">{t('telegram.botTitle')}</h2>
            {!status.configured && (
              <>
                <ol className="text-sm text-gray-600 list-decimal ps-5 space-y-1">
                  <li>{t('telegram.setupStep1')}</li>
                  <li>{t('telegram.setupStep2')}</li>
                  <li>{t('telegram.setupStep3')}</li>
                </ol>
                <div className="flex gap-2">
                  <Input type="password" dir="ltr" autoComplete="off" placeholder="123456789:AA…" value={token} onChange={(e) => setToken(e.target.value)} />
                  <Button disabled={busy || token.trim() === ''} onClick={() => act(async () => { const s = await invoke<TelegramStatus>('tg_save_token', { token }); setToken(''); return s; }, t('telegram.tokenAccepted'))}>{t('telegram.saveToken')}</Button>
                </div>
                <p className="text-xs text-gray-500">{t('telegram.envHint')}</p>
              </>
            )}
            {status.configured && (
              <div className="space-y-3">
                <div className="flex flex-wrap items-center gap-3 text-sm">
                  <span className={`px-2 py-0.5 rounded ${status.running ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-700'}`}>{status.running ? t('telegram.running') : t('telegram.stopped')}</span>
                  {status.bot_username && <bdi dir="ltr">@{status.bot_username}</bdi>}
                  <span className="text-gray-500">{t('telegram.tokenIn', { source: status.token_source ?? '' })}</span>
                  <div className="flex-1" />
                  {status.running ? (
                    <Button size="sm" variant="outline" disabled={busy} onClick={() => act(() => invoke<TelegramStatus>('tg_stop'))}>{t('telegram.stop')}</Button>
                  ) : (
                    <Button size="sm" disabled={busy} onClick={() => act(() => invoke<TelegramStatus>('tg_start'), t('telegram.botStarted'))}>{t('telegram.start')}</Button>
                  )}
                  <Button size="sm" variant="ghost" disabled={busy} onClick={() => act(() => invoke<TelegramStatus>('tg_clear_token'), t('telegram.tokenRemoved'))}>{t('telegram.removeToken')}</Button>
                </div>
                {status.last_error && <div dir="auto" className="bg-red-50 border border-red-200 rounded p-2 text-sm text-red-800">{status.last_error}</div>}
                <div className="text-xs text-gray-500">
                  {status.last_activity
                    ? t('telegram.activityLast', { updates: fmt.number(status.processed_updates), messages: fmt.number(status.sent_messages), time: fmt.dateTime(status.last_activity) })
                    : t('telegram.activityNone', { updates: fmt.number(status.processed_updates), messages: fmt.number(status.sent_messages) })}
                </div>
              </div>
            )}
          </div>
        )}

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <h2 className="font-medium">{t('telegram.introTitle')}</h2>
          <p className="text-sm text-gray-600">{t('telegram.introDesc')}</p>
          <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={firstName} onChange={(e) => setFirstName(e.target.checked)} /> {t('telegram.showFirstNames')}</label>
          <div className="grid grid-cols-2 gap-1">
            {fields.map((f) => (
              <label key={f.key} className="flex items-center gap-2 text-sm">
                <input type="checkbox" checked={shared.has(f.key)} onChange={(e) => setShared((prev) => { const n = new Set(prev); if (e.target.checked) n.add(f.key); else n.delete(f.key); return n; })} />
                <bdi dir="auto">{fieldLabel(f, locale)}</bdi>
                {f.sensitive && <span className="text-xs px-1.5 rounded bg-amber-100 text-amber-800">{t('profile.sensitive')}</span>}
              </label>
            ))}
          </div>
          <div className="flex justify-end"><Button size="sm" disabled={busy || !sharedDirty} onClick={saveSharing}>{t('common.save')}</Button></div>
        </div>

        {inbox && (inbox.unread.length > 0 || inbox.requests.length > 0) && (
          <div className="bg-white rounded-lg border p-4 space-y-3">
            <h2 className="font-medium">{t('telegram.attentionTitle')}</h2>
            {inbox.unread.map((u) => (
              <button key={u.profile_id} className="w-full text-start flex items-center gap-3 hover:bg-gray-50 rounded p-2" onClick={() => router.push(`/profile?id=${encodeURIComponent(u.profile_id)}`)}>
                <span className="flex-1"><bdi dir="auto">{u.name || t('profiles.unnamed')}</bdi></span>
                <span className="text-xs px-2 py-0.5 rounded bg-blue-100 text-blue-800">{t(u.unread > 1 ? 'telegram.newMessageMany' : 'telegram.newMessageOne', { count: fmt.number(u.unread) })}</span>
              </button>
            ))}
            {inbox.requests.map((r) => (
              <div key={r.id} className="flex items-center gap-3 p-2 bg-red-50 rounded">
                <button className="flex-1 text-start" onClick={() => router.push(`/profile?id=${encodeURIComponent(r.profile_id)}`)}>
                  {t('telegram.deleteRequest', { name: r.name || t('profiles.unnamed'), date: fmt.date(r.created_at) })}
                </button>
                <Button size="sm" variant="outline" onClick={async () => { try { await invoke('tg_resolve_request', { id: r.id }); await load(); } catch (e) { toast.error(`${e}`); } }}>{t('telegram.markDone')}</Button>
              </div>
            ))}
            <p className="text-xs text-gray-500">{t('telegram.markDoneNote')}</p>
          </div>
        )}
      </div>
    </div>
  );
}
