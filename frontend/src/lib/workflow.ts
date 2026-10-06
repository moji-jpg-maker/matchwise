import { Interest, MatchEvent, MatchStatus, Outcome } from '@/types/matchmaking';

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

export const STATUS_LABEL: Record<MatchStatus, string> = {
  identified: 'Identified',
  recommended: 'Recommended',
  reviewed: 'Reviewed',
  approved: 'Approved',
  introduction_proposed: 'Introduction proposed',
  both_interested: 'Both interested',
  contact_exchanged: 'Contact exchanged',
  conversation: 'Conversation',
  meeting: 'Meeting',
  feedback: 'Feedback',
  closed: 'Closed',
  rejected: 'Rejected',
  declined: 'Declined',
  stopped: 'Stopped',
};

/** Label for the button that moves a match to `to`. */
export const ACTION_LABEL: Record<MatchStatus, string> = {
  identified: 'Mark as identified',
  recommended: 'Recommend',
  reviewed: 'Mark as reviewed',
  approved: 'Approve',
  introduction_proposed: 'Propose introduction',
  both_interested: 'Both interested',
  contact_exchanged: 'Contact exchanged',
  conversation: 'Conversation started',
  meeting: 'Meeting held',
  feedback: 'Feedback received',
  closed: 'Close match',
  rejected: 'Reject',
  declined: 'Declined by a person',
  stopped: 'Stopped',
};

/** Moves that end or undo the match are styled as cautions. */
export const NEGATIVE: MatchStatus[] = ['rejected', 'declined', 'stopped'];

export const OUTCOME_LABEL: Record<Outcome, string> = {
  interested: 'Interested',
  not_interested: 'Not interested',
  first_conversation: 'First conversation',
  first_meeting: 'First meeting',
  continued: 'Continued',
  stopped: 'Stopped',
  relationship_formed: 'Relationship formed',
  married: 'Married',
  unknown: 'Unknown',
};

export const OUTCOMES = Object.keys(OUTCOME_LABEL) as Outcome[];

export const INTEREST_LABEL: Record<Interest, string> = {
  unknown: 'No answer yet',
  interested: 'Interested',
  not_interested: 'Not interested',
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

const label = (s: string | null) => (s ? STATUS_LABEL[s as MatchStatus] ?? s : '');

/** Plain-language line for the history timeline. Free-text reasons are shown separately, under the line. */
export function describeEvent(e: MatchEvent): string {
  switch (e.kind) {
    case 'created':
      return `Tracked as ${label(e.to_status).toLowerCase()}`;
    case 'status_changed':
      return `${label(e.from_status)} → ${label(e.to_status)}${e.actor === 'system' ? ' (automatic)' : ''}`;
    case 'response_recorded':
      return 'Interest recorded';
    case 'outcome_recorded':
      return `Outcome recorded: ${OUTCOME_LABEL[(e.detail ?? 'unknown') as Outcome] ?? e.detail}`;
    case 'info_requested':
      return 'More information requested';
    case 'hold_cleared':
      return 'Information request cleared';
    case 'note_added':
      return 'Note added';
    case 'hidden':
      return 'Hidden from lists';
    case 'unhidden':
      return 'Shown in lists again';
    case 'rescore':
      return 'Re-scored';
    case 'weights_adjusted':
      return 'Dimension weights adjusted and re-scored';
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
