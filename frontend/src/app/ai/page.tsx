'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { AiConfig, AiLogRow, AiSettings } from '@/types/matchmaking';

const PROVIDERS: { value: AiConfig['provider']; label: string; help: string }[] = [
  { value: 'none', label: 'Off', help: 'No AI features.' },
  { value: 'ollama', label: 'Ollama on this computer (recommended)', help: 'Runs on your machine: nothing leaves it. Install Ollama and a model first.' },
  { value: 'openai_compatible', label: 'OpenAI-compatible server', help: 'LM Studio, vLLM or llama.cpp on this computer, or a cloud service.' },
  { value: 'anthropic', label: 'Anthropic (Claude), cloud', help: 'Cloud: needs your acceptance of the cloud notice and an API key.' },
];

export default function AiSettingsPage() {
  const router = useRouter();
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
      apply(await invoke<AiSettings>('ai_get_settings'));
    } catch (e) {
      toast.error(`${e}`);
    }
  }, [apply]);
  useEffect(() => { void load(); }, [load]);

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

  if (!s) return <div className="p-6 text-gray-500">Loading…</div>;
  const defaultUrl = (p: string) => s.default_urls.find(([k]) => k === p)?.[1] ?? '';
  const urlIsCloud = provider !== 'none' && baseUrl.trim() !== '' && !/^https?:\/\/(localhost|127\.0\.0\.1|\[::1\])(:\d+)?(\/|$)/i.test(baseUrl.trim());

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-3xl mx-auto p-6 space-y-5">
        <div className="flex items-center gap-3">
          <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back"><ArrowLeft className="w-5 h-5" /></button>
          <div>
            <h1 className="text-2xl font-semibold">AI assistant</h1>
            <p className="text-sm text-gray-500">Reads free text and explains matches. It only suggests: you decide, and it never changes scores or statuses.</p>
          </div>
        </div>

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <h2 className="font-medium">Provider</h2>
          <div className="space-y-2">
            {PROVIDERS.map((p) => (
              <label key={p.value} className="flex items-start gap-2 text-sm">
                <input type="radio" className="mt-1" checked={provider === p.value} onChange={() => { setProvider(p.value); setBaseUrl(defaultUrl(p.value)); }} />
                <span><span className="font-medium">{p.label}</span><br /><span className="text-gray-500">{p.help}</span></span>
              </label>
            ))}
          </div>
          {provider !== 'none' && (
            <div className="grid grid-cols-3 gap-3 items-center">
              <label className="text-sm font-medium">Model</label>
              <Input className="col-span-2" placeholder={provider === 'ollama' ? 'for example llama3.1:8b (see `ollama list`)' : 'model name'} value={model} onChange={(e) => setModel(e.target.value)} />
              <label className="text-sm font-medium">Address</label>
              <Input className="col-span-2" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} />
            </div>
          )}
          {provider !== 'none' && !urlIsCloud && (
            <label className="flex items-start gap-2 text-sm">
              <input type="checkbox" className="mt-1" checked={localSensitive} onChange={(e) => setLocalSensitive(e.target.checked)} />
              <span>Let this local model read sensitive fields (health, finances, religion). They stay on this computer.</span>
            </label>
          )}
          {provider !== 'none' && (
            <div className={`text-sm rounded p-2 border ${urlIsCloud ? 'bg-amber-50 border-amber-200 text-amber-900' : 'bg-green-50 border-green-200 text-green-900'}`}>
              {urlIsCloud ? 'This address is outside your computer: it is treated as a cloud provider.' : 'This address is on your computer: nothing is sent elsewhere.'}
            </div>
          )}
          <div className="flex justify-end">
            <Button size="sm" disabled={busy} onClick={() => run(() => invoke<AiSettings>('ai_save_settings', { provider, model, baseUrl, localIncludeSensitive: localSensitive }), 'Saved')}>Save</Button>
          </div>
        </div>

        {s.config.provider !== 'none' && s.is_cloud && (
          <div className="bg-white rounded-lg border p-4 space-y-3">
            <h2 className="font-medium">Cloud provider: notice and consent</h2>
            <pre className="whitespace-pre-wrap text-sm bg-gray-50 border rounded p-3 font-sans">{s.consent_text}</pre>
            {s.consent && s.consent.version === s.consent_version ? (
              <div className="flex items-center gap-3 text-sm">
                <span className="px-2 py-0.5 rounded bg-green-100 text-green-800">Accepted {new Date(s.consent.at).toLocaleDateString()}{s.consent.include_sensitive ? ', sensitive fields included' : ''}</span>
                <Button size="sm" variant="outline" disabled={busy} onClick={() => run(() => invoke<AiSettings>('ai_set_cloud_consent', { accept: false, includeSensitive: false }), 'Consent withdrawn')}>Withdraw</Button>
              </div>
            ) : (
              <div className="space-y-2">
                <label className="flex items-start gap-2 text-sm">
                  <input type="checkbox" className="mt-1" checked={consentSensitive} onChange={(e) => setConsentSensitive(e.target.checked)} />
                  <span>Also allow sensitive fields (health, finances, religion) to be sent. Leave this off unless you really need it.</span>
                </label>
                <Button size="sm" disabled={busy} onClick={() => run(() => invoke<AiSettings>('ai_set_cloud_consent', { accept: true, includeSensitive: consentSensitive }), 'Consent recorded')}>I have read this and accept</Button>
              </div>
            )}
          </div>
        )}

        {s.config.provider !== 'none' && s.config.provider !== 'ollama' && (
          <div className="bg-white rounded-lg border p-4 space-y-2">
            <h2 className="font-medium">API key</h2>
            {s.has_key ? (
              <div className="flex items-center gap-3 text-sm">
                <span>A key is stored (in {s.key_source}). It is never shown.</span>
                <Button size="sm" variant="outline" disabled={busy} onClick={() => run(() => invoke<AiSettings>('ai_clear_key'), 'Key removed')}>Remove</Button>
              </div>
            ) : (
              <>
                <div className="flex gap-2">
                  <Input type="password" autoComplete="off" placeholder="API key" value={key} onChange={(e) => setKey(e.target.value)} />
                  <Button disabled={busy || key.trim() === ''} onClick={() => run(async () => { const r = await invoke<AiSettings>('ai_save_key', { key }); setKey(''); return r; }, 'Key stored')}>Save key</Button>
                </div>
                <p className="text-xs text-gray-500">Stored in this computer’s credential store, not in the database. On WSL, set MATCHWISE_AI_API_KEY instead. A local server that needs no key can leave this empty.</p>
              </>
            )}
          </div>
        )}

        <div className="bg-white rounded-lg border p-4 space-y-3">
          <div className="flex items-center gap-3">
            <span className={`text-xs px-2 py-0.5 rounded ${s.ready ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-700'}`}>{s.ready ? 'Ready' : 'Not ready'}</span>
            <span className="text-sm text-gray-600 flex-1">{s.ready ? (s.mode?.is_cloud ? 'Cloud mode: redacted text leaves this computer.' : 'Local mode: nothing leaves this computer.') : s.not_ready_reason}</span>
            <Button size="sm" variant="outline" disabled={busy || !s.ready} onClick={() => run(async () => { await invoke('ai_test_connection'); toast.success('The model answered'); })}>Test connection</Button>
          </div>
          <div className="text-sm text-gray-600 space-y-1">
            <p>What is always removed before text reaches a model: names (replaced by Person A / B), phone numbers, e-mail addresses, @handles, links, long numbers. Contact details, full names and dates of birth are never sent at all.</p>
            <p>Every request is listed below. For cloud requests the exact text that was sent is kept for 30 days.</p>
          </div>
          <Button size="sm" variant="ghost" onClick={async () => { setShowLog((v) => !v); if (!showLog) { try { setLog(await invoke<AiLogRow[]>('ai_log', { limit: 50 })); } catch (e) { toast.error(`${e}`); } } }}>
            {showLog ? 'Hide' : 'Show'} what was sent
          </Button>
          {showLog && (
            <ul className="space-y-2 text-sm">
              {log.length === 0 && <li className="text-gray-500">Nothing yet.</li>}
              {log.map((l, i) => (
                <li key={i} className="border rounded p-2">
                  <div className="flex flex-wrap items-center gap-2 text-xs">
                    <span className="font-medium">{l.kind}</span>
                    <span className={`px-1.5 rounded ${l.is_cloud ? 'bg-amber-100 text-amber-800' : 'bg-green-100 text-green-800'}`}>{l.is_cloud ? 'cloud' : 'local'}</span>
                    <span>{l.model}</span>
                    <span className="text-gray-400">{new Date(l.at).toLocaleString()}</span>
                    <span>{l.input_chars} chars in, {l.output_chars} out</span>
                    <span className={l.ok ? 'text-green-700' : 'text-red-700'}>{l.ok ? 'ok' : l.error}</span>
                  </div>
                  {l.prompt && <details className="mt-1"><summary className="cursor-pointer text-xs text-gray-600">The text that was sent</summary><pre className="whitespace-pre-wrap text-xs bg-gray-50 p-2 mt-1 max-h-64 overflow-auto font-sans">{l.prompt}</pre></details>}
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
