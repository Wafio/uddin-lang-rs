//! Calendar time as the reference keeps it: seconds since the start of year 1 and a nanosecond, in a location that
//! is UTC, the host's zone, or a fixed offset. The arithmetic is the reference's, wrapping where it wraps, so a
//! date far outside the years people write comes out the same as there. The host's zone is read through `jiff`.

use std::sync::OnceLock;

use jiff::Timestamp;
use jiff::tz::TimeZone;

const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 3600;
const SECONDS_PER_DAY: i64 = 86_400;
const ABSOLUTE_YEARS: u64 = 292_277_022_400;
const MARCH_THRU_DECEMBER: u32 = 306;

const UNIX_TO_INTERNAL: i64 = 62_135_596_800;
const INTERNAL_TO_UNIX: i64 = -UNIX_TO_INTERNAL;
const ABSOLUTE_TO_UNIX: i64 = -9_223_372_028_741_760_000;
const UNIX_TO_ABSOLUTE: i64 = 9_223_372_028_741_760_000;

/// The ends of time for a zone lookup.
const ALPHA: i64 = i64::MIN;
const OMEGA: i64 = i64::MAX;

#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) enum Loc {
    Utc,
    /// The zone of the host.
    Local,
    /// A zone of one offset in seconds, with no name.
    Fixed(i32),
    /// A zone of one offset in seconds that a layout named, as `MST` does.
    Named(i32, Box<str>),
}

#[derive(Clone, Debug)]
pub(super) struct Time {
    /// Seconds since 1 January of year 1.
    pub ext: i64,
    pub nsec: i32,
    pub loc: Loc,
}

/// What a zone says about an instant: the abbreviation, the offset east of UTC in seconds, and the span of
/// instants the same rule holds for.
pub(super) struct ZoneInfo {
    pub name: String,
    pub offset: i32,
    pub start: i64,
    pub end: i64,
}

fn host_zone() -> &'static TimeZone {
    static ZONE: OnceLock<TimeZone> = OnceLock::new();
    ZONE.get_or_init(TimeZone::system)
}

fn timestamp(unix: i64) -> Timestamp {
    let lo = Timestamp::MIN.as_second();
    let hi = Timestamp::MAX.as_second();
    Timestamp::from_second(unix.clamp(lo, hi)).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// Seconds in 400 years, after which the Gregorian calendar, and so every rule of a zone, repeats.
const CYCLE: i64 = 146_097 * SECONDS_PER_DAY;

/// The rule of the host's zone at `unix`. `jiff` knows the years up to 9999; for a later instant the rule is the
/// one of the same instant a whole number of 400-year cycles earlier.
pub(super) fn host_lookup(unix: i64) -> ZoneInfo {
    let last = Timestamp::MAX.as_second();
    let cycles = if unix > last {
        (unix - last + CYCLE - 1) / CYCLE
    } else {
        0
    };
    let shift = cycles * CYCLE;
    let unix = unix - shift;
    let zone = host_zone();
    let at = timestamp(unix);
    let info = zone.to_offset_info(at);
    let after = timestamp(unix.saturating_add(1));
    let moved = |t: i64| if cycles == 0 { t } else { t + shift };
    let start = zone
        .preceding(after)
        .next()
        .map_or(ALPHA, |t| moved(t.timestamp().as_second()));
    let end = zone
        .following(at)
        .next()
        .map_or(OMEGA, |t| moved(t.timestamp().as_second()));
    ZoneInfo {
        name: info.abbreviation().to_owned(),
        offset: info.offset().seconds(),
        start,
        end,
    }
}

/// The current time as unix seconds and nanoseconds.
pub(super) fn now() -> (i64, i32) {
    let ts = Timestamp::now();
    (ts.as_second(), ts.subsec_nanosecond())
}

/// The offset of the zone of the host that is called `name`, as the reference finds one: first a rule of that name
/// that is in force at the instant, and failing that any rule of that name. The rules looked at are the one in
/// force at `unix` and the few transitions on each side of it.
pub(super) fn host_lookup_name(name: &str, unix: i64) -> Option<i32> {
    let zone = host_zone();
    let at = timestamp(unix);
    let mut rules = vec![(host_lookup(unix).name, host_lookup(unix).offset)];
    for t in zone
        .following(at)
        .take(12)
        .chain(zone.preceding(at).take(12))
    {
        rules.push((t.abbreviation().to_owned(), t.offset().seconds()));
    }
    for (rule, offset) in &rules {
        if rule == name {
            let there = host_lookup(unix.saturating_sub(i64::from(*offset)));
            if there.name == name {
                return Some(there.offset);
            }
        }
    }
    rules
        .into_iter()
        .find(|(rule, _)| rule == name)
        .map(|(_, offset)| offset)
}

impl Loc {
    fn lookup(&self, unix: i64) -> ZoneInfo {
        match self {
            Loc::Utc => ZoneInfo {
                name: "UTC".to_owned(),
                offset: 0,
                start: ALPHA,
                end: OMEGA,
            },
            Loc::Local => host_lookup(unix),
            Loc::Fixed(offset) => ZoneInfo {
                name: String::new(),
                offset: *offset,
                start: ALPHA,
                end: OMEGA,
            },
            Loc::Named(offset, name) => ZoneInfo {
                name: name.to_string(),
                offset: *offset,
                start: ALPHA,
                end: OMEGA,
            },
        }
    }
}

impl Time {
    pub(super) fn from_unix(sec: i64, nsec: i32, loc: Loc) -> Time {
        Time {
            ext: sec.wrapping_add(UNIX_TO_INTERNAL),
            nsec,
            loc,
        }
    }

    pub(super) fn unix_sec(&self) -> i64 {
        self.ext.wrapping_add(INTERNAL_TO_UNIX)
    }

    /// The zone rule in force at this instant, in this time's location.
    pub(super) fn zone(&self) -> ZoneInfo {
        self.loc.lookup(self.unix_sec())
    }

    /// Seconds on the wall clock of the location since the start of the absolute era, given the offset there.
    pub(super) fn abs_sec(&self, offset: i32) -> u64 {
        self.unix_sec()
            .wrapping_add(i64::from(offset))
            .wrapping_add(UNIX_TO_ABSOLUTE) as u64
    }

    /// Year, month (1 to 12) and day of the month on the wall clock of the location.
    pub(super) fn date(&self) -> (i64, i64, i64) {
        let abs = self.abs_sec(self.zone().offset);
        days_to_date(abs / SECONDS_PER_DAY as u64)
    }

    pub(super) fn clock(&self) -> (i64, i64, i64) {
        clock(self.abs_sec(self.zone().offset))
    }

    /// Adds `secs` seconds, saturating where the reference does.
    fn add_sec(&mut self, secs: i64) {
        let sum = self.ext.wrapping_add(secs);
        if (sum > self.ext) == (secs > 0) {
            self.ext = sum;
        } else if secs > 0 {
            self.ext = i64::MAX;
        } else {
            self.ext = -i64::MAX;
        }
    }

    /// Adds a duration in nanoseconds.
    pub(super) fn add(mut self, d: i64) -> Time {
        let mut dsec = d / 1_000_000_000;
        let mut nsec = self.nsec + (d % 1_000_000_000) as i32;
        if nsec >= 1_000_000_000 {
            dsec += 1;
            nsec -= 1_000_000_000;
        } else if nsec < 0 {
            dsec -= 1;
            nsec += 1_000_000_000;
        }
        self.nsec = nsec;
        self.add_sec(dsec);
        self
    }

    /// Adds years, months and days on the wall clock, which can land on another day of the month and, in a zone
    /// that changes its offset, another hour.
    pub(super) fn add_date(self, years: i64, months: i64, days: i64) -> Time {
        let (year, month, day) = self.date();
        let (hour, min, sec) = self.clock();
        let nsec = i64::from(self.nsec);
        date(
            year.wrapping_add(years),
            month.wrapping_add(months),
            day.wrapping_add(days),
            hour,
            min,
            sec,
            nsec,
            self.loc,
        )
    }

    fn cmp_key(&self) -> (i64, i32) {
        (self.ext, self.nsec)
    }

    pub(super) fn before(&self, u: &Time) -> bool {
        self.cmp_key() < u.cmp_key()
    }

    pub(super) fn after(&self, u: &Time) -> bool {
        self.cmp_key() > u.cmp_key()
    }

    pub(super) fn equal(&self, u: &Time) -> bool {
        self.cmp_key() == u.cmp_key()
    }

    /// `self - u` in nanoseconds, the most negative or most positive duration when it does not fit.
    pub(super) fn sub(&self, u: &Time) -> i64 {
        let d = self
            .ext
            .wrapping_sub(u.ext)
            .wrapping_mul(1_000_000_000)
            .wrapping_add(i64::from(self.nsec) - i64::from(u.nsec));
        if u.clone().add(d).equal(self) {
            d
        } else if self.before(u) {
            i64::MIN
        } else {
            i64::MAX
        }
    }
}

/// The time of a date and clock reading on the wall of `loc`; out-of-range fields carry into the next ones.
#[allow(clippy::too_many_arguments)]
pub(super) fn date(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    min: i64,
    sec: i64,
    nsec: i64,
    loc: Loc,
) -> Time {
    let (year, m) = norm(year, month.wrapping_sub(1), 12);
    let month = m + 1;
    let (sec, nsec) = norm(sec, nsec, 1_000_000_000);
    let (min, sec) = norm(min, sec, 60);
    let (hour, min) = norm(hour, min, 60);
    let (day, hour) = norm(day, hour, 24);

    let abs_days = date_to_days(year, month, day) as i64;
    let mut unix = abs_days
        .wrapping_mul(SECONDS_PER_DAY)
        .wrapping_add(
            hour.wrapping_mul(SECONDS_PER_HOUR)
                .wrapping_add(min.wrapping_mul(SECONDS_PER_MINUTE))
                .wrapping_add(sec),
        )
        .wrapping_add(ABSOLUTE_TO_UNIX);

    let zone = loc.lookup(unix);
    let mut offset = zone.offset;
    if offset != 0 {
        let utc = unix.wrapping_sub(i64::from(offset));
        if utc < zone.start || utc >= zone.end {
            offset = loc.lookup(utc).offset;
        }
        unix = unix.wrapping_sub(i64::from(offset));
    }
    Time::from_unix(unix, nsec as i32, loc)
}

/// `lo` brought into `0..base` by carrying whole bases into `hi`.
fn norm(mut hi: i64, mut lo: i64, base: i64) -> (i64, i64) {
    if lo < 0 {
        let n = (lo.wrapping_neg().wrapping_sub(1)) / base + 1;
        hi = hi.wrapping_sub(n);
        lo = lo.wrapping_add(n.wrapping_mul(base));
    }
    if lo >= base {
        let n = lo / base;
        hi = hi.wrapping_add(n);
        lo = lo.wrapping_sub(n.wrapping_mul(base));
    }
    (hi, lo)
}

/// Days from the start of the absolute era to the given date; `month` is 1 to 12.
fn date_to_days(year: i64, month: i64, day: i64) -> u64 {
    let mut amonth = month as u32;
    let jan_feb = u32::from(amonth < 3);
    amonth += 12 * jan_feb;
    let y = (year as u64)
        .wrapping_sub(u64::from(jan_feb))
        .wrapping_add(ABSOLUTE_YEARS);
    let ayday = (979 * amonth - 2919) >> 5;
    let century = y / 100;
    let cyear = (y % 100) as u32;
    let cday = 1461 * cyear / 4;
    let century_days = 146_097u64.wrapping_mul(century) / 4;
    century_days.wrapping_add(i64::from(cday + ayday).wrapping_add(day).wrapping_sub(1) as u64)
}

/// The century, the year in it and the day of the year counted from March, of a day of the absolute era.
fn split_days(days: u64) -> (u64, u32, u32) {
    let d = days.wrapping_mul(4).wrapping_add(3);
    let century = d / 146_097;
    let cd = ((d % 146_097) as u32) | 3;
    let product = 2_939_745u64 * u64::from(cd);
    let cyear = (product >> 32) as u32;
    let lo = product as u32;
    (century, cyear, lo / 2_939_745 / 4)
}

fn split_yday(ayday: u32) -> (u32, i64) {
    let d = 2141 * ayday + 197_913;
    (d >> 16, 1 + i64::from((d & 0xFFFF) / 2141))
}

fn year_of(century: u64, cyear: u32, jan_feb: u32) -> i64 {
    century
        .wrapping_mul(100)
        .wrapping_sub(ABSOLUTE_YEARS)
        .wrapping_add(u64::from(cyear))
        .wrapping_add(u64::from(jan_feb)) as i64
}

pub(super) fn days_to_date(days: u64) -> (i64, i64, i64) {
    let (century, cyear, ayday) = split_days(days);
    let (amonth, day) = split_yday(ayday);
    let jan_feb = u32::from(ayday >= MARCH_THRU_DECEMBER);
    (
        year_of(century, cyear, jan_feb),
        i64::from(amonth) - i64::from(jan_feb) * 12,
        day,
    )
}

/// The year and the day of the year, counted from 1.
pub(super) fn days_to_year_yday(days: u64) -> (i64, i64) {
    let (century, cyear, ayday) = split_days(days);
    let jan_feb = u32::from(ayday >= MARCH_THRU_DECEMBER);
    let leap = i64::from(cyear % 4 == 0 && (cyear != 0 || century % 4 == 0));
    let yday =
        i64::from(ayday) + (1 + 31 + 28) + (leap & !i64::from(jan_feb)) - 365 * i64::from(jan_feb);
    (year_of(century, cyear, jan_feb), yday)
}

/// 0 for Sunday.
pub(super) fn weekday(days: u64) -> usize {
    ((days.wrapping_add(3)) % 7) as usize
}

pub(super) fn clock(abs: u64) -> (i64, i64, i64) {
    let mut sec = (abs % SECONDS_PER_DAY as u64) as i64;
    let hour = sec / SECONDS_PER_HOUR;
    sec -= hour * SECONDS_PER_HOUR;
    let min = sec / SECONDS_PER_MINUTE;
    sec -= min * SECONDS_PER_MINUTE;
    (hour, min, sec)
}

pub(super) fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

pub(super) fn days_in(month: i64, year: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Time {
        date(y, mo, d, h, mi, s, 0, Loc::Utc)
    }

    #[test]
    fn dates_round_trip_through_unix_time() {
        let t = utc(1970, 1, 1, 0, 0, 0);
        assert_eq!(t.unix_sec(), 0);
        let t = utc(2000, 2, 29, 23, 59, 59);
        assert_eq!(t.unix_sec(), 951_868_799);
        assert_eq!(t.date(), (2000, 2, 29));
        assert_eq!(t.clock(), (23, 59, 59));
        let t = utc(1, 1, 1, 0, 0, 0);
        assert_eq!(t.ext, 0);
        assert_eq!(t.date(), (1, 1, 1));
        let t = utc(9999, 12, 31, 23, 59, 59);
        assert_eq!(t.date(), (9999, 12, 31));
    }

    #[test]
    fn fields_out_of_range_carry() {
        assert_eq!(utc(2023, 14, 1, 0, 0, 0).date(), (2024, 2, 1));
        assert_eq!(utc(2023, 1, 32, 0, 0, 0).date(), (2023, 2, 1));
        assert_eq!(utc(2023, 3, 0, 0, 0, 0).date(), (2023, 2, 28));
        assert_eq!(utc(2023, 0, 1, 0, 0, 0).date(), (2022, 12, 1));
        assert_eq!(utc(2023, 1, 1, 25, 61, 61).clock(), (2, 2, 1));
    }

    #[test]
    fn adds_months_as_the_wall_clock_does() {
        let t = utc(2023, 1, 31, 12, 0, 0).add_date(0, 1, 0);
        assert_eq!(t.date(), (2023, 3, 3));
        let t = utc(2024, 2, 29, 0, 0, 0).add_date(1, 0, 0);
        assert_eq!(t.date(), (2025, 3, 1));
    }

    #[test]
    fn subtracts_with_saturation() {
        let a = utc(2023, 12, 25, 0, 0, 0);
        let b = utc(2023, 12, 24, 0, 0, 0);
        assert_eq!(a.sub(&b), 86_400 * 1_000_000_000);
        assert_eq!(
            utc(9999, 1, 1, 0, 0, 0).sub(&utc(1, 1, 1, 0, 0, 0)),
            i64::MAX
        );
        assert_eq!(
            utc(1, 1, 1, 0, 0, 0).sub(&utc(9999, 1, 1, 0, 0, 0)),
            i64::MIN
        );
    }

    #[test]
    fn counts_weekdays_and_days_of_the_year() {
        let days = |t: &Time| t.abs_sec(0) / SECONDS_PER_DAY as u64;
        let t = utc(2023, 12, 25, 0, 0, 0);
        assert_eq!(weekday(days(&t)), 1);
        assert_eq!(days_to_year_yday(days(&t)), (2023, 359));
        assert_eq!(
            days_to_year_yday(days(&utc(2024, 12, 31, 0, 0, 0))),
            (2024, 366)
        );
        assert_eq!(
            days_to_year_yday(days(&utc(2024, 1, 1, 0, 0, 0))),
            (2024, 1)
        );
    }
}
