import { describe, expect, test } from 'bun:test';

import { en } from '../../src/i18n/en';
import { fa } from '../../src/i18n/fa';
import { createFormatters, interpolate, normalizeSearch } from '../../src/i18n/format';
import { directionOf, lookup, translate } from '../../src/i18n';

describe('interpolate', () => {
  test('fills named params and leaves unknown ones', () => {
    expect(interpolate('Hello {name}, {missing}', { name: 'Sara' })).toBe('Hello Sara, {missing}');
  });

  test('returns the template when no params are given', () => {
    expect(interpolate('Hello {name}')).toBe('Hello {name}');
  });
});

describe('normalizeSearch', () => {
  test('unifies Arabic and Persian letter forms', () => {
    expect(normalizeSearch('علي')).toBe(normalizeSearch('علی'));
    expect(normalizeSearch('كتاب')).toBe(normalizeSearch('کتاب'));
  });

  test('maps Arabic-Indic and Persian digits to ASCII', () => {
    expect(normalizeSearch('٠١٢')).toBe('012');
    expect(normalizeSearch('۱۲۳')).toBe('123');
  });

  test('drops zero-width non-joiners and diacritics and lowercases', () => {
    expect(normalizeSearch('می\u200cخواهم')).toBe('میخواهم');
    expect(normalizeSearch('ABC')).toBe('abc');
  });
});

describe('formatters', () => {
  test('Persian numbers use Persian digits', () => {
    expect(createFormatters('fa').number(1234)).toMatch(/[۰-۹]/);
    expect(createFormatters('en').number(1234)).toBe('1,234');
  });

  test('Persian dates use the Persian calendar', () => {
    const text = createFormatters('fa').date(new Date('2026-03-21T12:00:00Z'));
    expect(text).toMatch(/[۰-۹]/);
    expect(text).not.toContain('2026');
  });

  test('percent rounds to a whole number', () => {
    expect(createFormatters('en').percent(0.256)).toBe('26%');
  });

  test('relative time picks a sensible unit', () => {
    const now = Date.parse('2026-01-10T00:00:00Z');
    const formatters = createFormatters('en');
    expect(formatters.relativeTime(now - 3 * 3600 * 1000, now)).toBe('3 hours ago');
    expect(formatters.relativeTime(now + 2 * 86400 * 1000, now)).toBe('in 2 days');
  });
});

describe('catalog helpers', () => {
  test('lookup resolves dotted keys and rejects non-strings', () => {
    expect(lookup(en, 'templateInfo.standard_meeting.name')).toBe('Standard Meeting Notes');
    expect(lookup(en, 'templateInfo.standard_meeting')).toBeUndefined();
    expect(lookup(en, 'nope.nothing')).toBeUndefined();
  });

  test('translate falls back to English and then to the key', () => {
    expect(translate('fa', 'templateInfo.standard_meeting.name')).toBe(
      lookup(fa, 'templateInfo.standard_meeting.name') as never,
    );
    expect(translate('en', 'no.such.key' as never)).toBe('no.such.key');
  });

  test('direction follows the locale', () => {
    expect(directionOf('fa')).toBe('rtl');
    expect(directionOf('en')).toBe('ltr');
  });

  test('every template has a Persian name and description', () => {
    for (const id of Object.keys(en.templateInfo)) {
      expect(lookup(fa, `templateInfo.${id}.name`)).toBeTruthy();
      expect(lookup(fa, `templateInfo.${id}.description`)).toBeTruthy();
    }
  });
});
