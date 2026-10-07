'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { OutboxItem, TelegramInvite, TelegramLink, TelegramMessage } from '@/types/matchmaking';

/** Invite a candidate to the bot, read and answer their messages, and see what has been sent to them. */
export function TelegramPanel({ profileId }: { profileId: string }) {
  const [link, setLink] = useState<TelegramLink | null>(null);
  const [invite, setInvite] = useState<TelegramInvite | null>(null);
  const [messages, setMessages] = useState<TelegramMessage[]>([]);
  const [outbox, setOutbox] = useState<OutboxItem[]>([]);
  const [text, setText] = useState('');
  const [showSent, setShowSent] = useState(false);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const l = await invoke<TelegramLink>('tg_get_link', { profileId });
      setLink(l);
      if (l.linked) {
        setMessages(await invoke<TelegramMessage[]>('tg_messages', { profileId }));
        setOutbox(await invoke<OutboxItem[]>('tg_outbox', { profileId }));
        if (l.unread > 0) {
          await invoke('tg_mark_read', { profileId });
        }
      }
    } catch (e) {
      toast.error(`${e}`);
    }
  }, [profileId]);

  useEffect(() => { void load(); }, [load]);
  // pick up replies while the panel is open
  useEffect(() => {
    const t = setInterval(() => { void load(); }, 8000);
    return () => clearInterval(t);
  }, [load]);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try { await fn(); } catch (e) { toast.error(`${e}`); } finally { setBusy(false); }
  };

  if (!link) return null;
  const copy = (s: string) => navigator.clipboard?.writeText(s).then(() => toast.success('Copied')).catch(() => toast.error('Could not copy'));

  return (
    <div className="bg-white rounded-lg border">
      <div className="p-4 border-b flex items-center gap-3">
        <div className="flex-1">
          <h2 className="font-medium">Telegram</h2>
          <p className="text-xs text-gray-500">Messages here pass through Telegram. Contact details are never shared automatically.</p>
        </div>
        {link.linked && (
          <span className={`text-xs px-2 py-0.5 rounded ${link.consented ? (link.notifications_enabled ? 'bg-green-100 text-green-800' : 'bg-amber-100 text-amber-800') : 'bg-amber-100 text-amber-800'}`}>
            {!link.consented ? 'waiting for them to accept the privacy notice' : link.notifications_enabled ? 'linked' : 'linked, notifications off'}
          </span>
        )}
      </div>

      {!link.linked && (
        <div className="p-4 space-y-3">
          <p className="text-sm text-gray-600">Not linked. Create an invitation and send it to this person yourself. It works once and expires in 48 hours.</p>
          <Button size="sm" disabled={busy} onClick={() => run(async () => setInvite(await invoke<TelegramInvite>('tg_create_invite', { profileId })))}>
            {link.open_invite ? 'Create a new invitation (cancels the old one)' : 'Create invitation'}
          </Button>
          {invite && (
            <div className="border rounded-md p-3 bg-gray-50 space-y-2 text-sm">
              {invite.link ? (
                <div className="flex items-center gap-2"><code className="flex-1 break-all">{invite.link}</code><Button size="sm" variant="outline" onClick={() => copy(invite.link!)}>Copy link</Button></div>
              ) : (
                <p className="text-amber-800">The bot has not been set up yet (Telegram page). Send the code below and ask them to type /start followed by the code to your bot.</p>
              )}
              <div className="flex items-center gap-2"><span>Code:</span><code className="font-semibold tracking-widest">{invite.code}</code><Button size="sm" variant="ghost" onClick={() => copy(invite.code)}>Copy</Button></div>
              <p className="text-xs text-gray-500">Shown only now: it cannot be displayed again. Expires {new Date(invite.expires_at).toLocaleString()}.</p>
            </div>
          )}
        </div>
      )}

      {link.linked && (
        <div className="p-4 space-y-3">
          <div className="text-sm text-gray-600">
            {link.username ? `@${link.username}` : 'Telegram account'} · linked {link.linked_at ? new Date(link.linked_at).toLocaleDateString() : ''}
          </div>
          <div className="max-h-64 overflow-y-auto space-y-2 border rounded-md p-3 bg-gray-50">
            {messages.length === 0 && <p className="text-sm text-gray-500">No messages yet.</p>}
            {messages.map((m) => (
              <div key={m.id} className={`text-sm flex ${m.direction === 'out' ? 'justify-end' : ''}`}>
                <div className={`max-w-[80%] rounded-lg px-3 py-1.5 ${m.direction === 'out' ? 'bg-blue-600 text-white' : 'bg-white border'}`}>
                  <div className="whitespace-pre-wrap">{m.text}</div>
                  <div className={`text-[10px] ${m.direction === 'out' ? 'text-blue-100' : 'text-gray-400'}`}>{new Date(m.created_at).toLocaleString()}{m.match_id ? ' · about a match' : ''}</div>
                </div>
              </div>
            ))}
          </div>
          {link.consented && (
            <div className="flex gap-2">
              <Input placeholder="Write to this person" value={text} onChange={(e) => setText(e.target.value)} />
              <Button size="sm" disabled={busy || text.trim() === '' || !link.notifications_enabled} onClick={() => run(async () => { await invoke('tg_send_message', { profileId, text }); setText(''); await load(); })}>Send</Button>
            </div>
          )}
          <div className="flex flex-wrap gap-2">
            <Button size="sm" variant="outline" disabled={busy || !link.consented} onClick={() => run(async () => { await invoke('tg_remind_profile', { profileId }); toast.success('Reminder queued'); await load(); })}>Remind to complete profile</Button>
            <Button size="sm" variant="ghost" onClick={() => setShowSent((v) => !v)}>{showSent ? 'Hide' : 'Show'} what was sent</Button>
            <div className="flex-1" />
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => run(async () => { await invoke('tg_unlink', { profileId }); setInvite(null); await load(); })}>Unlink</Button>
          </div>
          {showSent && (
            <ul className="space-y-2 text-sm border rounded-md p-3">
              {outbox.length === 0 && <li className="text-gray-500">Nothing yet.</li>}
              {outbox.map((o, i) => (
                <li key={i}>
                  <div className="flex items-center gap-2">
                    <span className="text-xs px-1.5 py-0.5 rounded bg-gray-100">{o.kind.replace(/_/g, ' ')}</span>
                    <span className={`text-xs ${o.status === 'sent' ? 'text-green-700' : o.status === 'pending' ? 'text-amber-700' : 'text-red-700'}`}>{o.status}</span>
                    <span className="text-xs text-gray-400">{new Date(o.sent_at ?? o.created_at).toLocaleString()}</span>
                  </div>
                  <div className="whitespace-pre-wrap text-gray-700">{o.text}</div>
                  {o.last_error && <div className="text-xs text-red-700">{o.last_error}</div>}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
