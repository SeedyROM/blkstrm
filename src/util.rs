use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastSeen<T>
where
    T: PartialOrd + Ord,
{
    pub value: T,
    pub timestamp: u128,
}

impl<T> LastSeen<T>
where
    T: PartialOrd + Ord,
{
    pub fn new(value: T) -> Self {
        Self {
            value,
            timestamp: SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        }
    }
}

pub fn is_sorted<T: IntoIterator>(t: T) -> bool
where
    <T as IntoIterator>::Item: std::cmp::PartialOrd,
{
    let mut iter = t.into_iter();

    if let Some(first) = iter.next() {
        iter.try_fold(first, |previous, current| {
            if previous > current {
                Err(())
            } else {
                Ok(current)
            }
        })
        .is_ok()
    } else {
        true
    }
}

pub fn no_sequential_duplicates<T: IntoIterator>(t: T) -> bool
where
    <T as IntoIterator>::Item: std::cmp::PartialEq,
{
    let mut iter = t.into_iter();

    if let Some(first) = iter.next() {
        iter.try_fold(first, |previous, current| {
            if previous == current {
                Err(())
            } else {
                Ok(current)
            }
        })
        .is_ok()
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;

    #[test]
    fn last_seen() {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let last_seen = LastSeen::new(1);
        assert_eq!(last_seen.value, 1);
        assert!(last_seen.timestamp >= now);
    }

    #[test]
    fn test_is_sorted() {
        let empty: Vec<i32> = vec![];
        let sorted = vec![1, 2, 3, 4, 5];
        let not_sorted = vec![5, 4, 3, 2, 1];

        assert!(is_sorted(empty));
        assert!(is_sorted(sorted));
        assert!(!is_sorted(not_sorted));
    }

    #[test]
    fn test_no_sequential_duplicates() {
        let empty: Vec<i32> = vec![];
        let no_duplicates = vec![1, 2, 3, 4, 5];
        let duplicates = vec![1, 1, 2, 3, 4, 5];

        assert!(no_sequential_duplicates(empty));
        assert!(no_sequential_duplicates(no_duplicates));
        assert!(!no_sequential_duplicates(duplicates));
    }
}
