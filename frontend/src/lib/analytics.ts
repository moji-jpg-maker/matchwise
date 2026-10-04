// Matchwise ships with NO telemetry.
//
// This module is an inert compatibility shim left over from the upstream code base: every method is a
// no-op, nothing is sent anywhere, nothing is stored, and no Tauri commands are invoked. It exists only so
// the ~100 remaining call sites keep compiling. Delete call sites (and then this file) as you touch them;
// do not add new calls.

export interface AnalyticsProperties {
  [key: string]: string;
}

export interface DeviceInfo {
  platform: string;
  os_version: string;
  architecture: string;
}

export interface UserSession {
  session_id: string;
  user_id: string;
  start_time: string;
  is_active: boolean;
}

/* eslint-disable @typescript-eslint/no-unused-vars, @typescript-eslint/no-explicit-any */
export class Analytics {
  static async init(): Promise<void> {}
  static async disable(): Promise<void> {}
  static async isEnabled(): Promise<boolean> { return false; }
  static async track(_eventName: string, _properties?: any): Promise<void> {}
  static async identify(_userId: string, _properties?: any): Promise<void> {}
  static async startSession(_userId: string): Promise<string | null> { return null; }
  static async endSession(): Promise<void> {}
  static async trackDailyActiveUser(): Promise<void> {}
  static async trackUserFirstLaunch(): Promise<void> {}
  static async isSessionActive(): Promise<boolean> { return false; }
  static async getPersistentUserId(): Promise<string> { return ''; }
  static async checkAndTrackFirstLaunch(): Promise<void> {}
  static async checkAndTrackDailyUsage(): Promise<void> {}
  static getCurrentUserId(): string | null { return null; }
  static async getPlatform(): Promise<string> { return 'unknown'; }
  static async getOSVersion(): Promise<string> { return 'unknown'; }
  static async getDeviceInfo(): Promise<DeviceInfo> {
    return { platform: 'unknown', os_version: 'unknown', architecture: 'unknown' };
  }
  static async calculateDaysSince(_dateKey: string): Promise<number | null> { return null; }
  static async updateMeetingCount(): Promise<void> {}
  static async getMeetingsCountToday(): Promise<number> { return 0; }
  static async hasUsedFeatureBefore(_featureName: string): Promise<boolean> { return false; }
  static async markFeatureUsed(_featureName: string): Promise<void> {}
  static async trackSessionStarted(_sessionId: string): Promise<void> {}
  static async trackSessionEnded(_sessionId: string): Promise<void> {}
  static async trackMeetingCompleted(_meetingId: string, _metrics: any): Promise<void> {}
  static async trackFeatureUsedEnhanced(_featureName: string, _properties?: any): Promise<void> {}
  static async trackCopy(_copyType: 'transcript' | 'summary', _properties?: any): Promise<void> {}
  static async trackMeetingStarted(_meetingId: string): Promise<void> {}
  static async trackRecordingStarted(_meetingId: string): Promise<void> {}
  static async trackRecordingStopped(_meetingId: string, _durationSeconds?: number): Promise<void> {}
  static async trackMeetingDeleted(_meetingId: string): Promise<void> {}
  static async trackSettingsChanged(_settingType: string, _newValue: string): Promise<void> {}
  static async trackFeatureUsed(_featureName: string): Promise<void> {}
  static async trackPageView(_pageName: string): Promise<void> {}
  static async trackButtonClick(_buttonName: string, _location?: string): Promise<void> {}
  static async trackError(_errorType: string, _errorMessage: string): Promise<void> {}
  static async trackAppStarted(): Promise<void> {}
  static async cleanup(): Promise<void> {}
  static reset(): void {}
  static async waitForInitialization(_timeout?: number): Promise<boolean> { return true; }
  static async trackBackendConnection(_success: boolean, _error?: string): Promise<void> {}
  static async trackTranscriptionError(_errorMessage: string): Promise<void> {}
  static async trackTranscriptionSuccess(_duration?: number): Promise<void> {}
  static async trackSummaryGenerationStarted(..._args: any[]): Promise<void> {}
  static async trackSummaryGenerationCompleted(..._args: any[]): Promise<void> {}
  static async trackSummaryRegenerated(_modelProvider: string, _modelName: string): Promise<void> {}
  static async trackModelChanged(_oldProvider: string, _oldModel: string, _newProvider: string, _newModel: string): Promise<void> {}
  static async trackCustomPromptUsed(_promptLength: number): Promise<void> {}
}

export default Analytics;
