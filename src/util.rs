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
