use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staleness {
    Fresh,
    Stale,
}

pub fn is_stale(binary_mtime: Option<SystemTime>, source_mtimes: &[SystemTime]) -> Staleness {
    let Some(binary) = binary_mtime else {
        return Staleness::Stale;
    };
    if source_mtimes.iter().any(|source| *source > binary) {
        Staleness::Stale
    } else {
        Staleness::Fresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn time(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn missing_binary_is_stale() {
        assert_eq!(is_stale(None, &[time(1)]), Staleness::Stale);
    }

    #[test]
    fn newer_binary_is_fresh() {
        assert_eq!(is_stale(Some(time(2)), &[time(1)]), Staleness::Fresh);
    }

    #[test]
    fn newer_source_is_stale() {
        assert_eq!(is_stale(Some(time(1)), &[time(2)]), Staleness::Stale);
    }

    #[test]
    fn equal_timestamps_are_fresh() {
        assert_eq!(is_stale(Some(time(1)), &[time(1)]), Staleness::Fresh);
    }

    #[test]
    fn empty_sources_are_fresh_when_binary_exists() {
        assert_eq!(is_stale(Some(time(1)), &[]), Staleness::Fresh);
    }
}
