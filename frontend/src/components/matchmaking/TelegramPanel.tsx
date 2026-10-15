'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useI18n } from '@/i18n';
import type { MessageKey } from '@/i18n/types';
import { OutboxItem, TelegramInvite, TelegramLink, TelegramMessage } from '@/types/matchmaking';

const OUTBOX_KIND_KEY: Record<string, MessageKey> = {
  both_interested: 'telegramPanel.kindBothInterested',
  info_request: 'telegramPanel.kindInfoRequest',
  intro_reminder: 'telegramPanel.kindIntroReminder',
  introduction: 'telegramPanel.kindIntroduction',
  matchmaker_message: 'telegramPanel.kindMatchmakerMessage',
  not_going_ahead: 'telegramPanel.kindNotGoingAhead',
  profile_reminder: 'telegramPanel.kindProfileReminder',
  questionnaire_assigned: 'telegramPanel.kindQuestionnaireAssigned',
  withdrawn: 'telegramPanel.kindWithdrawn',
};

const OUTBOX_STATUS_KEY: Record<OutboxItem['status'], MessageKey> = {
  pending: 'telegramPanel.statusPending',
  sent: 'telegramPanel.statusSent',
  failed: 'telegramPanel.statusFailed',
  cancelled: 'telegramPanel.statusCancelled',
};

/** Invite a candidate to the bot, read and answer their messages, and see what has been sent to them. */
export function TelegramPanel({ profileId }: { profileId: string }) {
  const { t, fmt } = useI18n();
  const [link, setLink] = useState<TelegramLink | null>(null);
  const [invite, setInvite] = useState<TelegramInvite | null>(null);
  const [messages, setMessages] = useState<TelegramMessage[]>([]);
  const [outbox, setOutbox] = useState<OutboxItem[]>([]);
  const [text, setText] = useState('');
  const [showSent, setShowSent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const l = await invoke<TelegramLink>('tg_get_link', { profileId });
      setLink(l);
      setLoadError(null);
      if (l.linked) {
        setMessages(await invoke<TelegramMessage[]>('tg_messages', { profileId }));
        setOutbox(await invoke<OutboxItem[]>('tg_outbox', { profileId }));
        if (l.unread > 0) {
          await invoke('tg_mark_read', { profileId });
        }
      }
    } catch (e) {
      setLoadError(`${e}`);
    }
  }, [profileId]);

  useEffect(() => { void load(); }, [load]);
  // pick up replies while the panel is open
  useEffect(() => {
    const timer = setInterval(() => { void load(); }, 8000);
    return () => clearInterval(timer);
  }, [load]);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try { await fn(); } catch (e) { toast.error(`${e}`); } finally { setBusy(false); }
  };

  if (!link) {
    return (
      <div className="bg-white rounded-lg border p-4 text-sm">
        <h2 className="font-medium mb-1">{t('telegramPanel.title')}</h2>
        {loadError ? (
          <div className="text-red-700 flex items-center gap-2">
            {t('telegramPanel.loadError', { error: loadError })}
            <Button size="sm" variant="outline" onClick={() => void load()}>{t('profiles.retry')}</Button>
          </div>
        ) : (
          <p className="text-gray-500">{t('common.loading')}</p>
        )}
      </div>
    );
  }
  const copy = (s: string) => navigator.clipboard?.writeText(s).then(() => toast.success(t('telegramPanel.copied'))).catch(() => toast.error(t('telegramPanel.copyFailed')));

  return (
    <div className="bg-white rounded-lg border">
      <div className="p-4 border-b flex items-center gap-3">
        <div className="flex-1">
          <h2 className="font-medium">{t('telegramPanel.title')}</h2>
          <p className="text-xs text-gray-500">{t('telegramPanel.note')}</p>
        </div>
        {link.linked && (
          <span className={`text-xs px-2 py-0.5 rounded ${link.consented ? (link.notifications_enabled ? 'bg-green-100 text-green-800' : 'bg-amber-100 text-amber-800') : 'bg-amber-100 text-amber-800'}`}>
            {!link.consented ? t('telegramPanel.waitingConsent') : link.notifications_enabled ? t('telegramPanel.linked') : t('telegramPanel.linkedMuted')}
          </span>
        )}
      </div>

      {!link.linked && (
        <div className="p-4 space-y-3">
          <p className="text-sm text-gray-600">{t('telegramPanel.notLinked')}</p>
          <Button size="sm" disabled={busy} onClick={() => run(async () => setInvite(await invoke<TelegramInvite>('tg_create_invite', { profileId })))}>
            {link.open_invite ? t('telegramPanel.inviteReplace') : t('telegramPanel.inviteCreate')}
          </Button>
          {invite && (
            <div className="border rounded-md p-3 bg-gray-50 space-y-2 text-sm">
              {invite.link ? (
                <div className="flex items-center gap-2"><code dir="ltr" className="flex-1 break-all">{invite.link}</code><Button size="sm" variant="outline" onClick={() => copy(invite.link!)}>{t('telegramPanel.copyLink')}</Button></div>
              ) : (
                <p className="text-amber-800">{t('telegramPanel.noBot')}</p>
              )}
              <div className="flex items-center gap-2"><span>{t('telegramPanel.code')}</span><code dir="ltr" className="font-semibold tracking-widest">{invite.code}</code><Button size="sm" variant="ghost" onClick={() => copy(invite.code)}>{t('telegramPanel.copy')}</Button></div>
              <p className="text-xs text-gray-500">{t('telegramPanel.shownOnce', { time: fmt.dateTime(invite.expires_at) })}</p>
            </div>
          )}
        </div>
      )}

      {link.linked && (
        <div className="p-4 space-y-3">
          <div className="text-sm text-gray-600">
            {t('telegramPanel.linkedOn', { who: link.username ? `@${link.username}` : t('telegramPanel.account'), date: link.linked_at ? fmt.date(link.linked_at) : '' })}
          </div>
          <div className="max-h-64 overflow-y-auto space-y-2 border rounded-md p-3 bg-gray-50">
            {messages.length === 0 && <p className="text-sm text-gray-500">{t('telegramPanel.noMessages')}</p>}
            {messages.map((m) => (
              <div key={m.id} className={`text-sm flex ${m.direction === 'out' ? 'justify-end' : ''}`}>
                <div className={`max-w-[80%] rounded-lg px-3 py-1.5 ${m.direction === 'out' ? 'bg-blue-600 text-white' : 'bg-white border'}`}>
                  <div dir="auto" className="whitespace-pre-wrap">{m.text}</div>
                  <div className={`text-[10px] ${m.direction === 'out' ? 'text-blue-100' : 'text-gray-400'}`}>{fmt.dateTime(m.created_at)}{m.match_id ? t('telegramPanel.aboutMatch') : ''}</div>
                </div>
              </div>
            ))}
          </div>
          {link.consented && (
            <div className="flex gap-2">
              <Input dir="auto" placeholder={t('telegramPanel.writePlaceholder')} value={text} onChange={(e) => setText(e.target.value)} />
              <Button size="sm" disabled={busy || text.trim() === '' || !link.notifications_enabled} onClick={() => run(async () => { await invoke('tg_send_message', { profileId, text }); setText(''); await load(); })}>{t('telegramPanel.send')}</Button>
            </div>
          )}
          <div className="flex flex-wrap gap-2">
            <Button size="sm" variant="outline" disabled={busy || !link.consented} onClick={() => run(async () => { await invoke('tg_remind_profile', { profileId }); toast.success(t('telegramPanel.reminderQueued')); await load(); })}>{t('telegramPanel.remind')}</Button>
            <Button size="sm" variant="ghost" onClick={() => setShowSent((v) => !v)}>{showSent ? t('telegramPanel.hideSent') : t('telegramPanel.showSent')}</Button>
            <div className="flex-1" />
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => run(async () => { await invoke('tg_unlink', { profileId }); setInvite(null); await load(); })}>{t('telegramPanel.unlink')}</Button>
          </div>
          {showSent && (
            <ul className="space-y-2 text-sm border rounded-md p-3">
              {outbox.length === 0 && <li className="text-gray-500">{t('telegramPanel.nothingYet')}</li>}
              {outbox.map((o, i) => (
                <li key={i}>
                  <div className="flex items-center gap-2">
                    <span className="text-xs px-1.5 py-0.5 rounded bg-gray-100">{OUTBOX_KIND_KEY[o.kind] ? t(OUTBOX_KIND_KEY[o.kind]) : o.kind.replace(/_/g, ' ')}</span>
                    <span className={`text-xs ${o.status === 'sent' ? 'text-green-700' : o.status === 'pending' ? 'text-amber-700' : 'text-red-700'}`}>{t(OUTBOX_STATUS_KEY[o.status])}</span>
                    <span className="text-xs text-gray-400">{fmt.dateTime(o.sent_at ?? o.created_at)}</span>
                  </div>
                  <div dir="auto" className="whitespace-pre-wrap text-gray-700">{o.text}</div>
                  {o.last_error && <div dir="auto" className="text-xs text-red-700">{o.last_error}</div>}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
