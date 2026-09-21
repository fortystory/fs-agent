//! The daily ledger (spec §17): one UTC day's usage, derived from the session
//! files.
//!
//! There is **no** ledger state file. A vendor documents no rate-limit headers,
//! so the only honest way to know how much of a rolling window has been spent is
//! to add up what the streams already record — which also means the ledger cannot
//! drift from the sessions it summarizes, and `prune` cannot leave it lying.
//!
//! The day boundary is UTC, and it is the **event's own timestamp** that decides
//! which day a record belongs to; a session that spans midnight contributes to
//! both days. Session files are filtered by mtime first, which is an optimization
//! and never a correctness risk: an event written on a day makes the file's
//! mtime that day or later, so a candidate filter of `mtime >= day start` cannot
//! drop a file that holds an event from that day.
//!
//! Cost is deliberately absent: attributing usage to a model needs the roster,
//! which lives in the caller's configuration and not on the stream, so the ledger
//! counts the tokens a quota window is measured in and leaves money to the views
//! that know which model each speaker answered with.

use std::io;
use std::time::SystemTime;

use chrono::{NaiveDate, Utc};

use crate::events::{read_events, EventPayload, Usage};

use super::store::{modified, SessionStore};

/// One UTC day's totals, summed from the session files.
#[derive(Debug, Clone, PartialEq)]
pub struct DayLedger {
    /// The day, in UTC.
    pub day: NaiveDate,
    /// How many sessions recorded usage on that day. A session counts once,
    /// however many calls it made.
    pub sessions: usize,
    /// How many `UsageRecorded` events fell on that day: the call count.
    pub calls: usize,
    /// The day's token totals.
    pub usage: Usage,
}

impl DayLedger {
    /// The day's total tokens: the number a vendor's rolling window is measured
    /// in.
    pub fn tokens(&self) -> u64 {
        self.usage.total_tokens()
    }
}

/// Sum one UTC day's usage out of every session file under `store`.
///
/// A session whose stream cannot be read is skipped rather than failing the
/// ledger: this is a display aggregation over files the user may have moved or
/// truncated by hand, and one unreadable file must not hide the rest of the day.
pub fn for_day(store: &SessionStore, day: NaiveDate) -> io::Result<DayLedger> {
    // A file written before the day started cannot hold an event from it, so
    // this is a candidate filter and never a correctness dependency. It goes
    // through the store's own `modified`, whose "unreadable is ancient"
    // convention is the one the rest of the store sorts by.
    let day_start = SystemTime::from(
        day.and_hms_opt(0, 0, 0)
            .expect("midnight exists on every day")
            .and_utc(),
    );
    let mut ledger = DayLedger {
        day,
        sessions: 0,
        calls: 0,
        usage: Usage::default(),
    };

    for stored in store.list_all()? {
        if modified(&stored.log_path) < day_start {
            continue;
        }
        let Ok(events) = read_events(&stored.log_path) else {
            continue;
        };
        let mut recorded = false;
        for event in events {
            if event.at.date_naive() != day {
                continue;
            }
            if let EventPayload::UsageRecorded { usage } = event.payload {
                ledger.calls += 1;
                ledger.usage.accumulate(usage);
                recorded = true;
            }
        }
        if recorded {
            ledger.sessions += 1;
        }
    }

    Ok(ledger)
}

/// Today's ledger, in UTC.
pub fn today(store: &SessionStore) -> io::Result<DayLedger> {
    for_day(store, Utc::now().date_naive())
}
