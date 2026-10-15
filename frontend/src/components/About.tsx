import React, { useState, useEffect } from "react";
import { getVersion } from '@tauri-apps/api/app';
import { useT } from "@/i18n";

export function About() {
    const t = useT();
    const [currentVersion, setCurrentVersion] = useState<string>('');

    useEffect(() => {
        getVersion().then(setCurrentVersion).catch(console.error);
    }, []);

    return (
        <div className="p-4 space-y-4 max-h-[80vh] overflow-y-auto">
            <div className="text-center">
                <h1 className="text-xl font-bold text-gray-900">Matchwise</h1>
                {currentVersion && <span dir="ltr" className="inline-block text-sm text-gray-500">v{currentVersion}</span>}
                <p className="mt-2 text-sm text-gray-600">
                    {t('about.tagline')}
                </p>
            </div>

            <div className="text-sm text-gray-700 space-y-2">
                <p>
                    {t('about.privacy')}
                </p>
                <p>
                    {t('about.ai')}
                </p>
            </div>

            <div className="pt-2 border-t border-gray-200 text-center">
                <p className="text-xs text-gray-400">
                    {t('about.fork')}
                </p>
            </div>
        </div>
    );
}
