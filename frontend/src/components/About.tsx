import React, { useState, useEffect } from "react";
import { getVersion } from '@tauri-apps/api/app';

export function About() {
    const [currentVersion, setCurrentVersion] = useState<string>('');

    useEffect(() => {
        getVersion().then(setCurrentVersion).catch(console.error);
    }, []);

    return (
        <div className="p-4 space-y-4 max-h-[80vh] overflow-y-auto">
            <div className="text-center">
                <h1 className="text-xl font-bold text-gray-900">Matchwise</h1>
                {currentVersion && <span className="text-sm text-gray-500">v{currentVersion}</span>}
                <p className="mt-2 text-sm text-gray-600">
                    Privacy-first, AI-assisted matchmaking for human matchmakers.
                </p>
            </div>

            <div className="text-sm text-gray-700 space-y-2">
                <p>
                    Your profile data stays on this device. Matchwise sends no analytics and does not check for
                    updates automatically.
                </p>
                <p>
                    AI features are optional and assistive: they never overwrite information entered by you or a
                    matchmaker.
                </p>
            </div>

            <div className="pt-2 border-t border-gray-200 text-center">
                <p className="text-xs text-gray-400">
                    Started as a fork of Meetily by Zackriya Solutions (MIT License).
                </p>
            </div>
        </div>
    );
}
