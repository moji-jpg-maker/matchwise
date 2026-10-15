'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Progress } from '@/components/ui/progress';
import { AiConfig, AiLogRow, AiSettings } from '@/types/matchmaking';
import { BuiltInAIAPI } from '@/lib/builtin-ai';
import { formatDownloadDetail } from '@/lib/download-progress';
import { useBuiltInDownload } from '@/lib/use-builtin-download';
import { useI18n } from '@/i18n';
import type { MessageKey } from '@/i18n/types';

const PROVIDER_HELP_KEY: Record<string, MessageKey> = {
  none: 'ai.providerHelp.none',
  builtin: 'ai.providerHelp.builtin',
  ollama: 'ai.providerHelp.ollama',
  openai_compatible: 'ai.providerHelp.openaiCompatible',
  anthropic: 'ai.providerHelp.anthropic',
};

const READINESS_KEY: Record<AiSettings['readiness'], MessageKey> = {
  ready: 'ai.readiness.ready',
  disabled: 'ai.readiness.disabled',
  model_missing: 'ai.readiness.modelMissing',
  needs_key: 'ai.readiness.needsKey',
  needs_consent: 'ai.readiness.needsConsent',
  invalid: 'ai.readiness.invalid',
};

export function AiSettingsPanel() {
  const { t, fmt, locale } = useI18n();
  const [s, setS] = useState<AiSettings | null>(null);
  const [provider, setProvider] = useState<AiConfig['provider']>('none');
  const [model, setModel] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [localSensitive, setLocalSensitive] = useState(true);
  const [key, setKey] = useState('');
  const [consentSensitive, setConsentSensitive] = useState(false);
  const [log, setLog] = useState<AiLogRow[]>([]);
  const [showLog, setShowLog] = useState(false);
  const [busy, setBusy] = useState(false);

  const apply = useCallback((v: AiSettings) => {
    setS(v);
    setProvider(v.config.provider);
    setModel(v.config.model);
    setBaseUrl(v.config.base_url);
    setLocalSensitive(v.config.local_include_sensitive);
    setConsentSensitive(v.consent?.include_sensitive ?? false);
  }, []);

  const load = useCallback(async () => {
    try {
      apply(await invoke<AiSettings>('ai_get_settings', { locale }));
    } catch (e) {
      toast.error(`${e}`);
    }
  }, [apply, locale]);
  useEffect(() => { void load(); }, [load]);

  const refreshAfterDownload = useCallback((finishedModel: string, status: 'completed' | 'error' | 'cancelled') => {
    void (async () => {
      try {
        // Same readiness refresh the built-in model manager uses, then reload matchmaking settings.
        if (status === 'completed') await BuiltInAIAPI.isModelReady(finishedModel, true);
      } catch { /* the settings reload below still reports the real state */ }
      void load();
    })();
  }, [load]);

  const { download, start: startDownload, cancel: cancelDownload } = useBuiltInDownload(model, refreshAfterDownload);
  const downloadingSelected = download !== null && download.model === model;
  const dlProgress = downloadingSelected && download ? Math.round(download.progress) : null;


  const run = async (fn: () => Promise<AiSettings | void>, ok?: string) => {
    setBusy(true);
    try {
      const r = await fn();
      if (r) apply(r);
      if (ok) toast.success(ok);
    } catch (e) {
      toast.error(`${e}`);
    } finally {
      setBusy(false);
    }
  };

  if (!s) return <div className="p-6 text-gray-500">{t('ai.loading')}</div>;
  const defaultUrl = (p: string) => s.default_urls.find(([k]) => k === p)?.[1] ?? '';
  const isBuiltin = provider === 'builtin';
  const urlIsCloud = provider !== 'none' && !isBuiltin && baseUrl.trim() !== '' && !/^https?:\/\/(localhost|127\.0\.0\.1|\[::1\])(:\d+)?(\/|$)/i.test(baseUrl.trim());
  const builtinModels = s.builtin_models;
  const selectedBuiltin = builtinModels.find((m) => m.id === model);
  const recommended = builtinModels.find((m) => m.recommended) ?? builtinModels[0];

  return (
    <div className="max-w-3xl space-y-5">
      <div>
        <h2 className="text-2xl font-semibold">{t('ai.title')}</h2>
        <p className="text-sm text-gray-500">{t('ai.intro')}</p>
      </div>

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <h2 className="font-medium">{t('ai.providerHeading')}</h2>
          <div className="space-y-2">
            {s.catalog.map((p) => (
              <label key={p.id} className="flex items-start gap-2 text-sm">
                <input type="radio" className="mt-1" checked={provider === p.id} onChange={() => { setProvider(p.id); setBaseUrl(defaultUrl(p.id)); if (p.model_from_list && !builtinModels.some((m) => m.id === model) && recommended) setModel(recommended.id); }} />
                <span><span className="font-medium">{p.label}</span><br /><span className="text-gray-500">{PROVIDER_HELP_KEY[p.id] ? t(PROVIDER_HELP_KEY[p.id]) : ''}</span></span>
              </label>
            ))}
          </div>
          {isBuiltin && (
            <div className="grid grid-cols-3 gap-3 items-center">
              <label className="text-sm font-medium">{t('ai.model')}</label>
              <select className="col-span-2 border rounded h-9 px-2 text-sm bg-white" value={model} onChange={(e) => setModel(e.target.value)}>
                {!selectedBuiltin && <option value={model}>{model}</option>}
                {builtinModels.map((m) => (
                  <option key={m.id} value={m.id}>{m.label} ({t('ai.sizeGb', { size: fmt.number(Math.round(m.size_mb / 100) / 10) })}), {m.id === model && dlProgress !== null ? t('ai.statusDownloading') : m.file === 'downloaded' ? t('ai.statusDownloaded') : t('ai.statusNotDownloaded')}{m.recommended ? `, ${t('ai.recommended')}` : ''}</option>
                ))}
              </select>
              <span />
              <div className="col-span-2 flex items-center gap-3 text-sm">
                {selectedBuiltin && selectedBuiltin.file === 'downloaded' && dlProgress === null ? (
                  <span className="text-green-700">{t('ai.downloadedReady')}</span>
                ) : (
                  <>
                    <Button size="sm" variant="outline" disabled={download !== null || !selectedBuiltin} onClick={() => startDownload(model)}>
                      {dlProgress !== null ? t('ai.downloadingPercent', { percent: fmt.percent(dlProgress / 100) }) : t('ai.downloadModel')}
                    </Button>
                    {download !== null && (
                      <Button size="sm" variant="ghost" onClick={cancelDownload}>{t('ai.cancel')}</Button>
                    )}
                    <span className="text-gray-500">{selectedBuiltin?.note ?? t('ai.chooseModel')}</span>
                  </>
                )}
              </div>
              {download && (
                <div className="col-span-3 space-y-1">
                  <Progress value={download.progress} aria-label={t('ai.downloadingAria', { model: download.model })} aria-valuetext={formatDownloadDetail(download, t, fmt)} />
                  <div className="text-xs text-gray-500">{formatDownloadDetail(download, t, fmt)}</div>
                </div>
              )}
            </div>
          )}
          {provider !== 'none' && !isBuiltin && (
            <div className="grid grid-cols-3 gap-3 items-center">
              <label className="text-sm font-medium">{t('ai.model')}</label>
              <Input className="col-span-2" dir="ltr" placeholder={provider === 'ollama' ? t('ai.modelPlaceholderOllama') : t('ai.modelPlaceholder')} value={model} onChange={(e) => setModel(e.target.value)} />
              <label className="text-sm font-medium">{t('ai.address')}</label>
              <Input className="col-span-2" dir="ltr" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} />
            </div>
          )}
          {provider !== 'none' && !urlIsCloud && (
            <label className="flex items-start gap-2 text-sm">
              <input type="checkbox" className="mt-1" checked={localSensitive} onChange={(e) => setLocalSensitive(e.target.checked)} />
              <span>{t('ai.localSensitive')}</span>
            </label>
          )}
          {provider !== 'none' && (
            <div className={`text-sm rounded p-2 border ${urlIsCloud ? 'bg-amber-50 border-amber-200 text-amber-900' : 'bg-green-50 border-green-200 text-green-900'}`}>
              {urlIsCloud ? t('ai.urlCloud') : (isBuiltin ? t('ai.builtinLocal') : t('ai.urlLocal'))}
            </div>
          )}
          <div className="flex justify-end">
            <Button size="sm" disabled={busy} onClick={() => run(() => invoke<AiSettings>('ai_save_settings', { provider, model, baseUrl, localIncludeSensitive: localSensitive, locale }), t('ai.saved'))}>{t('ai.save')}</Button>
          </div>
        </div>

        {s.config.provider !== 'none' && s.is_cloud && (
          <div className="bg-white rounded-lg border p-4 space-y-3">
            <h2 className="font-medium">{t('ai.cloudNoticeTitle')}</h2>
            <pre className="whitespace-pre-wrap text-sm bg-gray-50 border rounded p-3 font-sans">{s.consent_text}</pre>
            {s.consent && s.consent.version === s.consent_version ? (
              <div className="flex items-center gap-3 text-sm">
                <span className="px-2 py-0.5 rounded bg-green-100 text-green-800">{t('ai.consentAccepted', { date: fmt.date(s.consent.at) })}{s.consent.include_sensitive ? t('ai.consentSensitiveIncluded') : ''}</span>
                <Button size="sm" variant="outline" disabled={busy} onClick={() => run(() => invoke<AiSettings>('ai_set_cloud_consent', { accept: false, includeSensitive: false, locale }), t('ai.consentWithdrawn'))}>{t('ai.withdraw')}</Button>
              </div>
            ) : (
              <div className="space-y-2">
                <label className="flex items-start gap-2 text-sm">
                  <input type="checkbox" className="mt-1" checked={consentSensitive} onChange={(e) => setConsentSensitive(e.target.checked)} />
                  <span>{t('ai.consentSensitiveOption')}</span>
                </label>
                <Button size="sm" disabled={busy} onClick={() => run(() => invoke<AiSettings>('ai_set_cloud_consent', { accept: true, includeSensitive: consentSensitive, locale }), t('ai.consentRecorded'))}>{t('ai.consentAccept')}</Button>
              </div>
            )}
          </div>
        )}

        {s.config.provider !== 'none' && s.config.provider !== 'ollama' && s.config.provider !== 'builtin' && (
          <div className="bg-white rounded-lg border p-4 space-y-2">
            <h2 className="font-medium">{t('ai.apiKeyTitle')}</h2>
            {s.has_key ? (
              <div className="flex items-center gap-3 text-sm">
                <span>{t('ai.keyStored', { source: s.key_source ?? '' })}</span>
                <Button size="sm" variant="outline" disabled={busy} onClick={() => run(() => invoke<AiSettings>('ai_clear_key', { locale }), t('ai.keyRemoved'))}>{t('ai.remove')}</Button>
              </div>
            ) : (
              <>
                <div className="flex gap-2">
                  <Input type="password" autoComplete="off" dir="ltr" placeholder={t('ai.apiKeyPlaceholder')} value={key} onChange={(e) => setKey(e.target.value)} />
                  <Button disabled={busy || key.trim() === ''} onClick={() => run(async () => { const r = await invoke<AiSettings>('ai_save_key', { key, locale }); setKey(''); return r; }, t('ai.keyStoredToast'))}>{t('ai.saveKey')}</Button>
                </div>
                <p className="text-xs text-gray-500">{t('ai.keyHelp')}</p>
              </>
            )}
          </div>
        )}

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <div className="flex items-center gap-3">
            <span className={`text-xs px-2 py-0.5 rounded ${s.ready ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-700'}`}>{t(READINESS_KEY[s.readiness])}</span>
            <span className="text-sm text-gray-600 flex-1">{s.ready ? (s.mode?.is_cloud ? t('ai.modeCloud') : t('ai.modeLocal')) : s.not_ready_reason}</span>
            <Button size="sm" variant="outline" disabled={busy || !s.ready} onClick={() => run(async () => { await invoke('ai_test_connection', { locale }); toast.success(t('ai.modelAnswered')); })}>{t('ai.testConnection')}</Button>
          </div>
          <div className="text-sm text-gray-600 space-y-1">
            <p>{t('ai.privacyRemoved')}</p>
            <p>{t('ai.privacyLog')}</p>
          </div>
          <Button size="sm" variant="ghost" onClick={async () => { setShowLog((v) => !v); if (!showLog) { try { setLog(await invoke<AiLogRow[]>('ai_log', { limit: 50 })); } catch (e) { toast.error(`${e}`); } } }}>
            {showLog ? t('ai.hideSent') : t('ai.showSent')}
          </Button>
          {showLog && (
            <ul className="space-y-2 text-sm">
              {log.length === 0 && <li className="text-gray-500">{t('ai.logEmpty')}</li>}
              {log.map((l, i) => (
                <li key={i} className="border rounded p-2">
                  <div className="flex flex-wrap items-center gap-2 text-xs">
                    <span className="font-medium">{l.kind}</span>
                    <span className={`px-1.5 rounded ${l.is_cloud ? 'bg-amber-100 text-amber-800' : 'bg-green-100 text-green-800'}`}>{l.is_cloud ? t('ai.logCloud') : t('ai.logLocal')}</span>
                    <span>{l.model}</span>
                    <span className="text-gray-400">{fmt.dateTime(l.at)}</span>
                    <span>{t('ai.logChars', { input: fmt.number(l.input_chars), output: fmt.number(l.output_chars) })}</span>
                    <span className={l.ok ? 'text-green-700' : 'text-red-700'}>{l.ok ? t('ai.logOk') : l.error}</span>
                  </div>
                  {l.prompt && <details className="mt-1"><summary className="cursor-pointer text-xs text-gray-600">{t('ai.logSentText')}</summary><pre className="whitespace-pre-wrap text-xs bg-gray-50 p-2 mt-1 max-h-64 overflow-auto font-sans">{l.prompt}</pre></details>}
                </li>
              ))}
            </ul>
          )}
        </div>
    </div>
  );
}
