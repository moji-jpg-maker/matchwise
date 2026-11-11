//! Jalali (Solar Hijri) to Gregorian conversion, built in so the rule tier needs no new dependency.
//!
//! The arithmetic is the standard break-year algorithm used by `jalaali-js`: the Jalali calendar is defined
//! against the March equinox, and the 33-year cycles between the tabulated break years decide which Esfands
//! have 30 days. Only the forward direction is needed: a form gives a Jalali date, the profile stores ISO
//! Gregorian, and nothing ever converts back.

/// A Gregorian date `(year, month, day)`.
pub type Gregorian = (i32, u32, u32);

struct JalCal {
    /// Years since the last leap year in the current cycle; `0` means this year is leap.
    leap: i32,
    /// Gregorian year of the Jalali new year.
    gy: i32,
    /// Gregorian day of March on which the Jalali year starts.
    march: i32,
}

/// The first day of each Jalali cycle. A year outside the table cannot be converted.
const BREAKS: [i32; 20] = [
    -61, 9, 38, 199, 426, 686, 756, 818, 1111, 1181, 1210, 1635, 2060, 2097, 2192, 2262, 2324, 2394, 2456, 3178,
];

/// Whether `jy` is a leap Jalali year, so its Esfand has 30 days.
pub fn is_leap_year(jy: i32) -> bool {
    jal_cal(jy).is_some_and(|cal| cal.leap == 0)
}

/// How many days month `jm` of year `jy` has, or `None` when the year or month is out of range.
pub fn month_length(jy: i32, jm: u32) -> Option<u32> {
    let cal = jal_cal(jy)?;
    Some(match jm {
        1..=6 => 31,
        7..=11 => 30,
        12 if cal.leap == 0 => 30,
        12 => 29,
        _ => return None,
    })
}

/// The Gregorian date of a Jalali `(year, month, day)`, or `None` when the date does not exist.
pub fn to_gregorian(jy: i32, jm: u32, jd: u32) -> Option<Gregorian> {
    let cal = jal_cal(jy)?;
    if jd < 1 || jd > month_length(jy, jm)? {
        return None;
    }
    let (jm, jd) = (jm as i32, jd as i32);
    let day_number = g2d(cal.gy, 3, cal.march) + (jm - 1) * 31 - div(jm, 7) * (jm - 7) + jd - 1;
    Some(d2g(day_number))
}

fn jal_cal(jy: i32) -> Option<JalCal> {
    if jy < BREAKS[0] || jy >= *BREAKS.last()? {
        return None;
    }
    let mut leap_j = -14;
    let mut jp = BREAKS[0];
    let mut jump = 0;
    for jm in BREAKS.iter().copied().skip(1) {
        jump = jm - jp;
        if jy < jm {
            break;
        }
        leap_j += div(jump, 33) * 8 + div(jump % 33, 4);
        jp = jm;
    }
    let mut n = jy - jp;
    leap_j += div(n, 33) * 8 + div(n % 33 + 3, 4);
    if jump % 33 == 4 && jump - n == 4 {
        leap_j += 1;
    }
    let gy = jy + 621;
    let leap_g = div(gy, 4) - div((div(gy, 100) + 1) * 3, 4) - 150;
    let march = 20 + leap_j - leap_g;
    if jump - n < 6 {
        n = n - jump + div(jump + 4, 33) * 33;
    }
    let mut leap = (n + 1) % 33 - 1;
    leap %= 4;
    if leap == -1 {
        leap = 4;
    }
    Some(JalCal { leap, gy, march })
}

/// Truncating division, matching the reference implementation for negative years too.
fn div(a: i32, b: i32) -> i32 {
    a / b
}

/// Days since the Gregorian epoch.
fn g2d(gy: i32, gm: i32, gd: i32) -> i32 {
    let d = div((gy + div(gm - 8, 6) + 100100) * 1461, 4) + div(153 * (gm + 9).rem_euclid(12) + 2, 5) + gd - 34840408;
    d - div(div(gy + 100100 + div(gm - 8, 6), 100) * 3, 4) + 752
}

fn d2g(jdn: i32) -> Gregorian {
    let mut j = 4 * jdn + 139361631;
    j += div(div(4 * jdn + 183187720, 146097) * 3, 4) * 4 - 3908;
    let i = div(j % 1461, 4) * 5 + 308;
    let gd = div(i % 153, 5) + 1;
    let gm = div(i, 153) % 12 + 1;
    let gy = div(j, 1461) - 100100 + div(8 - gm, 6);
    (gy, gm as u32, gd as u32)
}

#[cfg(test)]
mod tests {
    use super::{is_leap_year, month_length, to_gregorian};

    #[test]
    fn known_dates_convert() {
        assert_eq!(to_gregorian(1368, 1, 1), Some((1989, 3, 21)));
        assert_eq!(to_gregorian(1399, 12, 30), Some((2021, 3, 20)));
        assert_eq!(to_gregorian(1403, 12, 30), Some((2025, 3, 20)));
        assert_eq!(to_gregorian(1400, 1, 1), Some((2021, 3, 21)));
        assert_eq!(to_gregorian(1367, 11, 29), Some((1989, 2, 18)));
    }

    #[test]
    fn leap_years_decide_whether_esfand_has_thirty_days() {
        assert!(is_leap_year(1399));
        assert!(is_leap_year(1403));
        assert!(!is_leap_year(1402));
        assert_eq!(month_length(1403, 12), Some(30));
        assert_eq!(month_length(1402, 12), Some(29));
        assert_eq!(month_length(1402, 6), Some(31));
        assert_eq!(month_length(1402, 7), Some(30));
    }

    #[test]
    fn impossible_dates_are_rejected() {
        assert_eq!(to_gregorian(1402, 12, 30), None);
        assert_eq!(to_gregorian(1402, 13, 1), None);
        assert_eq!(to_gregorian(1402, 7, 31), None);
        assert_eq!(to_gregorian(1402, 1, 32), None);
        assert_eq!(to_gregorian(-70, 1, 1), None);
    }
}
