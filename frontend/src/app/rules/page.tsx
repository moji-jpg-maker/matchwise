'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Plus } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { RuleSetSummary } from '@/types/matchmaking';

export default function RuleSetsPage() {
  const router = useRouter();
  const [sets, setSets] = useState<RuleSetSummary[]>([]);
  const [showArchived, setShowArchived] = useState(false);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    try {
      setSets(await invoke<RuleSetSummary[]>('mm_list_rule_sets', { includeArchived: showArchived }));
    } catch (e) {
      toast.error(`Could not load rule sets: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [showArchived]);

  useEffect(() => { void load(); }, [load]);

  const toggleArchive = async (s: RuleSetSummary) => {
    try {
      await invoke('mm_archive_rule_set', { id: s.id, archived: !s.archived });
      await load();
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  return (
    <div className="min-h-screen bg-gray-50">
      <div className="max-w-4xl mx-auto p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/')} className="p-2 rounded-lg hover:bg-gray-200" aria-label="Back">
              <ArrowLeft className="w-5 h-5" />
            </button>
            <div>
              <h1 className="text-2xl font-semibold">Rule sets</h1>
              <p className="text-sm text-gray-500">Each rule set is a matchmaking program. Every save creates a new version.</p>
            </div>
          </div>
          <Button onClick={() => router.push('/rule-set?id=new')}>
            <Plus className="w-4 h-4 mr-1" /> New rule set
          </Button>
        </div>

        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} />
          Show archived
        </label>

        <div className="bg-white rounded-lg border divide-y">
          {sets.length === 0 && !loading && <div className="p-6 text-center text-sm text-gray-500">No rule sets yet.</div>}
          {sets.map((s) => (
            <div key={s.id} className="p-4 flex items-center gap-4">
              <button className="flex-1 min-w-0 text-left" onClick={() => router.push(`/rule-set?id=${encodeURIComponent(s.id)}`)}>
                <div className="font-medium">
                  {s.name} {s.archived && <span className="ml-2 text-xs px-2 py-0.5 rounded bg-gray-200">archived</span>}
                </div>
                <div className="text-sm text-gray-500 truncate">{s.description || 'No description'}</div>
              </button>
              <div className="text-sm text-gray-600 w-40 text-right">
                v{s.current_version} · {s.rule_count} {s.rule_count === 1 ? 'rule' : 'rules'}
              </div>
              <Button variant="ghost" size="sm" onClick={() => toggleArchive(s)}>
                {s.archived ? 'Restore' : 'Archive'}
              </Button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
