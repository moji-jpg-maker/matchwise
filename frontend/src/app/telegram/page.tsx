'use client';

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { FieldDef, TelegramInbox, TelegramStatus, kindName } from '@/types/matchmaking';

export default function TelegramSettingsPage() {
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
  useEffect(() => {
    const t = setInterval(async () => {
      try { setStatus(await invoke<TelegramStatus>('tg_get_status')); } catch { /* ignore */ }
    }, 5000);
    return () => clearInterval(t);
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
    }, 'Introduction settings saved');

  const sharedDirty = useMemo(
    () => status != null && (firstName !== status.show_first_name || shared.size !== status.introduction_fields.length || status.introduction_fields.some((k) => !shared.has(k))),
    [firstName, shared, status]
  );

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-3xl mx-auto p-6 space-y-5">
        <div className="flex items-center gap-3">
          <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back"><ArrowLeft className="w-5 h-5" /></button>
          <div>
            <h1 className="text-2xl font-semibold">Telegram</h1>
            <p className="text-sm text-gray-500">Let candidates complete profiles, answer introductions and message you through your own bot.</p>
          </div>
        </div>

        <div className="bg-amber-50 border border-amber-200 rounded-lg p-4 text-sm space-y-1">
          <div className="font-medium">What stays on this computer, and what does not</div>
          <p>Profiles, scores and rules stay in the encrypted database on this computer. Everything sent to or received from Telegram, including introduction cards and anything a candidate types, passes through Telegram’s servers.</p>
          <p>The bot never shows stored sensitive details, scores or rules to anyone, and never shares contact details: you do that yourself. It only works while Matchwise is running.</p>
        </div>

        {status && (
          <div className="bg-white rounded-lg border p-4 space-y-3">
            <h2 className="font-medium">Your bot</h2>
            {!status.configured && (
              <>
                <ol className="text-sm text-gray-600 list-decimal pl-5 space-y-1">
                  <li>In Telegram, open <b>@BotFather</b> and send <code>/newbot</code>.</li>
                  <li>Choose a name and a username; BotFather replies with a token.</li>
                  <li>Paste the token here. It is stored in this computer’s credential store, never in the database.</li>
                </ol>
                <div className="flex gap-2">
                  <Input type="password" autoComplete="off" placeholder="123456789:AA…" value={token} onChange={(e) => setToken(e.target.value)} />
                  <Button disabled={busy || token.trim() === ''} onClick={() => act(async () => { const s = await invoke<TelegramStatus>('tg_save_token', { token }); setToken(''); return s; }, 'Token accepted')}>Save token</Button>
                </div>
                <p className="text-xs text-gray-500">On WSL or a machine without a credential store, set the environment variable MATCHWISE_TELEGRAM_TOKEN instead.</p>
              </>
            )}
            {status.configured && (
              <div className="space-y-3">
                <div className="flex flex-wrap items-center gap-3 text-sm">
                  <span className={`px-2 py-0.5 rounded ${status.running ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-700'}`}>{status.running ? 'Running' : 'Stopped'}</span>
                  {status.bot_username && <span>@{status.bot_username}</span>}
                  <span className="text-gray-500">token in {status.token_source}</span>
                  <div className="flex-1" />
                  {status.running ? (
                    <Button size="sm" variant="outline" disabled={busy} onClick={() => act(() => invoke<TelegramStatus>('tg_stop'))}>Stop</Button>
                  ) : (
                    <Button size="sm" disabled={busy} onClick={() => act(() => invoke<TelegramStatus>('tg_start'), 'Bot started')}>Start</Button>
                  )}
                  <Button size="sm" variant="ghost" disabled={busy} onClick={() => act(() => invoke<TelegramStatus>('tg_clear_token'), 'Token removed')}>Remove token</Button>
                </div>
                {status.last_error && <div className="bg-red-50 border border-red-200 rounded p-2 text-sm text-red-800">{status.last_error}</div>}
                <div className="text-xs text-gray-500">
                  {status.processed_updates} updates handled, {status.sent_messages} messages sent this session{status.last_activity ? `, last activity ${new Date(status.last_activity).toLocaleTimeString()}` : ''}.
                </div>
              </div>
            )}
          </div>
        )}

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <h2 className="font-medium">What an introduction shows</h2>
          <p className="text-sm text-gray-600">When you propose an introduction, each person receives a card about the other with only the facts ticked here. Contact details, full names and dates of birth can never be shared this way.</p>
          <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={firstName} onChange={(e) => setFirstName(e.target.checked)} /> Show first names (otherwise “Someone”)</label>
          <div className="grid grid-cols-2 gap-1">
            {fields.map((f) => (
              <label key={f.key} className="flex items-center gap-2 text-sm">
                <input type="checkbox" checked={shared.has(f.key)} onChange={(e) => setShared((prev) => { const n = new Set(prev); if (e.target.checked) n.add(f.key); else n.delete(f.key); return n; })} />
                {f.label}
                {f.sensitive && <span className="text-xs px-1.5 rounded bg-amber-100 text-amber-800">sensitive</span>}
              </label>
            ))}
          </div>
          <div className="flex justify-end"><Button size="sm" disabled={busy || !sharedDirty} onClick={saveSharing}>Save</Button></div>
        </div>

        {inbox && (inbox.unread.length > 0 || inbox.requests.length > 0) && (
          <div className="bg-white rounded-lg border p-4 space-y-3">
            <h2 className="font-medium">Needs your attention</h2>
            {inbox.unread.map((u) => (
              <button key={u.profile_id} className="w-full text-left flex items-center gap-3 hover:bg-gray-50 rounded p-2" onClick={() => router.push(`/profile?id=${encodeURIComponent(u.profile_id)}`)}>
                <span className="flex-1">{u.name || 'Unnamed profile'}</span>
                <span className="text-xs px-2 py-0.5 rounded bg-blue-100 text-blue-800">{u.unread} new message{u.unread > 1 ? 's' : ''}</span>
              </button>
            ))}
            {inbox.requests.map((r) => (
              <div key={r.id} className="flex items-center gap-3 p-2 bg-red-50 rounded">
                <button className="flex-1 text-left" onClick={() => router.push(`/profile?id=${encodeURIComponent(r.profile_id)}`)}>
                  {r.name || 'Unnamed profile'} asked for their data to be deleted ({new Date(r.created_at).toLocaleDateString()})
                </button>
                <Button size="sm" variant="outline" onClick={async () => { try { await invoke('tg_resolve_request', { id: r.id }); await load(); } catch (e) { toast.error(`${e}`); } }}>Mark as done</Button>
              </div>
            ))}
            <p className="text-xs text-gray-500">“Mark as done” only records that you handled the request. Delete the profile itself from its page.</p>
          </div>
        )}
      </div>
    </div>
  );
}
