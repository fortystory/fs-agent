//! 日账本（spec §17）：一个 UTC 自然日的用量，从会话文件里推出来。
//!
//! **没有**账本状态文件。供应商不公开任何限流响应头，所以想知道一个滚动窗口花了多少，唯一诚
//! 实的办法是把流里已经记下的加起来 —— 这也意味着账本不可能与它所汇总的会话产生偏差，`prune`
//! 也留不下它。
//!
//! 日界是 UTC，而决定一条记录属于哪一天的，是**事件自己的时间戳**：跨过午夜的一场会话给两天各
//! 记一份。会话文件先按 mtime 过滤，这是优化、绝不是正确性依赖：某天写下的事件让文件的 mtime
//! 落在当天或更晚，所以 `mtime >= day start` 这个候选过滤不可能丢掉一个装着当天事件的文件。
//!
//! 成本刻意缺席：把用量归到某个模型头上需要名册，而名册在调用方的配置里、不在流上，所以账本只
//! 数配额窗口赖以度量的那个 token 数，把钱留给知道每位讨论者用哪个模型作答的那些视图。

use std::io;
use std::time::SystemTime;

use chrono::{NaiveDate, Utc};

use crate::events::{EventPayload, Usage, read_events};

use super::store::{SessionStore, modified};

/// 一个 UTC 自然日的合计，从各会话文件加总而来。
#[derive(Debug, Clone, PartialEq)]
pub struct DayLedger {
    /// 日期，UTC。
    pub day: NaiveDate,
    /// 这一天有多少场会话记下了用量。一场会话只算一次，不管它调了多少次。
    pub sessions: usize,
    /// 这一天落下了多少条 `UsageRecorded` 事件：即调用次数。
    pub calls: usize,
    /// 当天的 token 合计。
    pub usage: Usage,
}

impl DayLedger {
    /// 当天的 token 总数：供应商滚动窗口赖以度量的那个数。
    pub fn tokens(&self) -> u64 {
        self.usage.total_tokens()
    }
}

/// 把 `store` 下每个会话文件里某一个 UTC 自然日的用量加总。
///
/// 读不出事件流的会话跳过即可，不让账本失败：这是在用户可能手工挪过或截过的文件上做展示性汇
/// 总，一个读不了的文件不该把这一天剩下的部分一并藏起来。
pub fn for_day(store: &SessionStore, day: NaiveDate) -> io::Result<DayLedger> {
    // 在当天开始之前写下的文件不可能装着当天的事件，所以这是候选过滤、绝不是正确性依赖。它走
    // store 自己的 `modified`，那条「读不出就算古老」的约定正是 store 其余部分排序时用的那
    // 一条。
    let day_start = SystemTime::from(day.and_hms_opt(0, 0, 0).expect("每一天都有午夜").and_utc());
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

/// 今天的账本，按 UTC。
pub fn today(store: &SessionStore) -> io::Result<DayLedger> {
    for_day(store, Utc::now().date_naive())
}
