import { Interest, MatchEvent, MatchStatus, Outcome } from '@/types/matchmaking';
import type { Translate } from '@/lib/conditions';
import type { MessageKey } from '@/i18n/types';

export const PIPELINE: MatchStatus[] = [
  'identified',
  'recommended',
  'reviewed',
  'approved',
  'introduction_proposed',
  'both_interested',
  'contact_exchanged',
  'conversation',
  'meeting',
  'feedback',
];

export const TERMINAL: MatchStatus[] = ['closed', 'rejected', 'declined', 'stopped'];

export const STATUS_KEY: Record<MatchStatus, MessageKey> = {
  identified: 'workflow.status.identified',
  recommended: 'workflow.status.recommended',
  reviewed: 'workflow.status.reviewed',
  approved: 'workflow.status.approved',
  introduction_proposed: 'workflow.status.introduction_proposed',
  both_interested: 'workflow.status.both_interested',
  contact_exchanged: 'workflow.status.contact_exchanged',
  conversation: 'workflow.status.conversation',
  meeting: 'workflow.status.meeting',
  feedback: 'workflow.status.feedback',
  closed: 'workflow.status.closed',
  rejected: 'workflow.status.rejected',
  declined: 'workflow.status.declined',
  stopped: 'workflow.status.stopped',
};

export const ACTION_KEY: Record<MatchStatus, MessageKey> = {
  identified: 'workflow.action.identified',
  recommended: 'workflow.action.recommended',
  reviewed: 'workflow.action.reviewed',
  approved: 'workflow.action.approved',
  introduction_proposed: 'workflow.action.introduction_proposed',
  both_interested: 'workflow.action.both_interested',
  contact_exchanged: 'workflow.action.contact_exchanged',
  conversation: 'workflow.action.conversation',
  meeting: 'workflow.action.meeting',
  feedback: 'workflow.action.feedback',
  closed: 'workflow.action.closed',
  rejected: 'workflow.action.rejected',
  declined: 'workflow.action.declined',
  stopped: 'workflow.action.stopped',
};

/** Moves that end or undo the match are styled as cautions. */
export const NEGATIVE: MatchStatus[] = ['rejected', 'declined', 'stopped'];

export const OUTCOME_KEY: Record<Outcome, MessageKey> = {
  interested: 'workflow.outcome.interested',
  not_interested: 'workflow.outcome.not_interested',
  first_conversation: 'workflow.outcome.first_conversation',
  first_meeting: 'workflow.outcome.first_meeting',
  continued: 'workflow.outcome.continued',
  stopped: 'workflow.outcome.stopped',
  relationship_formed: 'workflow.outcome.relationship_formed',
  married: 'workflow.outcome.married',
  unknown: 'workflow.outcome.unknown',
};

export const OUTCOMES = Object.keys(OUTCOME_KEY) as Outcome[];

export const INTEREST_KEY: Record<Interest, MessageKey> = {
  unknown: 'workflow.interest.unknown',
  interested: 'workflow.interest.interested',
  not_interested: 'workflow.interest.not_interested',
};

export const STATUS_CHIP: Record<MatchStatus, string> = {
  identified: 'bg-gray-100 text-gray-700',
  recommended: 'bg-blue-100 text-blue-800',
  reviewed: 'bg-blue-100 text-blue-800',
  approved: 'bg-indigo-100 text-indigo-800',
  introduction_proposed: 'bg-purple-100 text-purple-800',
  both_interested: 'bg-purple-100 text-purple-800',
  contact_exchanged: 'bg-teal-100 text-teal-800',
  conversation: 'bg-teal-100 text-teal-800',
  meeting: 'bg-teal-100 text-teal-800',
  feedback: 'bg-teal-100 text-teal-800',
  closed: 'bg-green-100 text-green-800',
  rejected: 'bg-red-100 text-red-800',
  declined: 'bg-red-100 text-red-800',
  stopped: 'bg-orange-100 text-orange-800',
};

const statusText = (t: Translate, status: string | null) =>
  status ? (status in STATUS_KEY ? t(STATUS_KEY[status as MatchStatus]) : status) : '';

/** Plain-language line for the history timeline. Free-text reasons are shown separately, under the line. */
export function describeEvent(e: MatchEvent, t: Translate): string {
  switch (e.kind) {
    case 'created':
      return t('workflow.event.created', { status: statusText(t, e.to_status) });
    case 'status_changed':
      return `${statusText(t, e.from_status)} → ${statusText(t, e.to_status)}${e.actor === 'system' ? t('workflow.event.automatic') : ''}`;
    case 'response_recorded':
      return t('workflow.event.responseRecorded');
    case 'outcome_recorded': {
      const outcome = e.detail ?? 'unknown';
      return t('workflow.event.outcomeRecorded', { outcome: outcome in OUTCOME_KEY ? t(OUTCOME_KEY[outcome as Outcome]) : outcome });
    }
    case 'info_requested':
      return t('workflow.event.infoRequested');
    case 'hold_cleared':
      return t('workflow.event.holdCleared');
    case 'note_added':
      return t('workflow.event.noteAdded');
    case 'hidden':
      return t('workflow.event.hidden');
    case 'unhidden':
      return t('workflow.event.unhidden');
    case 'rescore':
      return t('workflow.event.rescore');
    case 'weights_adjusted':
      return t('workflow.event.weightsAdjusted');
    default:
      return e.kind;
  }
}

/** Free text belonging to an event, if any (reasons, overrides, the information that was requested). */
export function eventText(e: MatchEvent): string | null {
  if (!e.detail) return null;
  if (['status_changed', 'info_requested'].includes(e.kind)) return e.detail;
  return null;
}
