use time::OffsetDateTime;

pub(in crate::ui) fn now_local() -> OffsetDateTime {
    OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc())
}

pub(in crate::ui) fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}
