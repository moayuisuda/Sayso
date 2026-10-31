//! 协议与数据文件中的时间：ISO-8601 UTC，精确到秒，按文本排序即按时间排序。

use time::{format_description::FormatItem, macros::format_description, OffsetDateTime};

const FORMAT: &[FormatItem<'static>] = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]Z");

pub fn now_iso() -> String {
    OffsetDateTime::now_utc().format(FORMAT).expect("UTC time is formattable")
}
