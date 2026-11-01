//! A Development inspector's log and the rows of it its filter shows, kept up to date as rows come and go so a repaint doesn't filter the whole log again.

use std::{
    cell::{Ref, RefCell},
    collections::VecDeque,
    ops::Index,
    time::Instant,
};

/// Which client's log an inspector reads, whether a read is under way, and when the next may start.
#[derive(Default)]
pub struct Poller {
    pid: Option<u32>,
    reading: bool,
    next_poll: Option<Instant>,
}

impl Poller {
    /// Whether a read of `pid`'s log should start now; `enabled` is the inspector's own switch.
    pub fn due(&self, enabled: bool, pid: u32, now: Instant) -> bool {
        enabled
            && (!self.reading || self.pid != Some(pid))
            && self.next_poll.is_none_or(|at| now >= at)
    }

    /// Marks a read of `pid`'s log started. True when it is another client than last time: what was kept starts over.
    pub fn start(&mut self, pid: u32) -> bool {
        let other = self.pid != Some(pid);
        self.pid = Some(pid);
        self.reading = true;
        other
    }

    /// Marks the read over, the next allowed from `next_poll`. False when the answer is for a client that was left.
    pub fn finish(&mut self, pid: u32, next_poll: Option<Instant>) -> bool {
        self.reading = false;
        self.next_poll = next_poll;
        self.pid == Some(pid)
    }

    /// When the next read may start, if one is waiting for that.
    pub fn next_poll(&self) -> Option<Instant> {
        self.next_poll
    }
}

/// Rows numbered from the first ever pushed, so the numbers stay put as the oldest go.
pub struct Log<T> {
    rows: VecDeque<T>,
    // the number of the front row
    first: u64,
}

impl<T> Default for Log<T> {
    fn default() -> Self {
        Self {
            rows: VecDeque::new(),
            first: 0,
        }
    }
}

impl<T> Log<T> {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.rows.iter()
    }

    /// For changes that leave what a filter sees alone.
    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut T> {
        self.rows.iter_mut()
    }

    pub fn push(&mut self, row: T) {
        self.rows.push_back(row);
    }

    pub fn clear(&mut self) {
        self.first += self.rows.len() as u64;
        self.rows.clear();
    }

    /// Clears the log when a page starting at `first` is not newer than its newest row (`behind(newest, first)`):
    /// the client has a new agent with a new log, so what was kept is stale.
    pub fn restart_if_behind<F>(&mut self, first: &F, behind: impl Fn(&T, &F) -> bool) {
        if self.rows.back().is_some_and(|last| behind(last, first)) {
            self.clear();
        }
    }

    /// Drops the oldest rows until at most `keep` are left.
    pub fn keep(&mut self, keep: usize) {
        while self.rows.len() > keep {
            self.rows.pop_front();
            self.first += 1;
        }
    }

    fn end(&self) -> u64 {
        self.first + self.rows.len() as u64
    }

    fn row(&self, number: u64) -> &T {
        &self.rows[(number - self.first) as usize]
    }
}

impl<T> Extend<T> for Log<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, rows: I) {
        self.rows.extend(rows);
    }
}

/// The numbers of the rows a filter shows, for the filter `key` it last ran with.
pub struct Filtered<K> {
    cache: RefCell<Cache<K>>,
}

struct Cache<K> {
    key: Option<K>,
    // the number of the first row not looked at yet
    seen: u64,
    shown: VecDeque<u64>,
}

impl<K> Default for Filtered<K> {
    fn default() -> Self {
        Self {
            cache: RefCell::new(Cache {
                key: None,
                seen: 0,
                shown: VecDeque::new(),
            }),
        }
    }
}

impl<K: PartialEq> Filtered<K> {
    /// The rows of `log` that `shows`, filtering only the rows new since the last call unless `key` changed.
    pub fn view<'a, T>(
        &'a self,
        log: &'a Log<T>,
        key: K,
        shows: impl Fn(&T) -> bool,
    ) -> View<'a, T, K> {
        let mut cache = self.cache.borrow_mut();
        // a log that ends before what was seen is another log: filter it all
        let from = if cache.key.as_ref() == Some(&key) && cache.seen <= log.end() {
            while cache
                .shown
                .front()
                .is_some_and(|&number| number < log.first)
            {
                cache.shown.pop_front();
            }
            cache.seen.max(log.first)
        } else {
            cache.key = Some(key);
            cache.shown.clear();
            log.first
        };
        for number in from..log.end() {
            if shows(log.row(number)) {
                cache.shown.push_back(number);
            }
        }
        cache.seen = log.end();
        drop(cache);
        View {
            log,
            shown: self.cache.borrow(),
        }
    }
}

/// The shown rows, oldest first.
pub struct View<'a, T, K> {
    log: &'a Log<T>,
    shown: Ref<'a, Cache<K>>,
}

impl<'a, T, K> View<'a, T, K> {
    pub fn len(&self) -> usize {
        self.shown.shown.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.shown.shown.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&'a T> {
        self.shown
            .shown
            .get(index)
            .map(|&number| self.log.row(number))
    }

    #[cfg(test)]
    pub fn range(&self, rows: std::ops::Range<usize>) -> impl Iterator<Item = &'a T> + '_ {
        self.shown
            .shown
            .range(rows)
            .map(|&number| self.log.row(number))
    }

    #[cfg(test)]
    pub fn iter(&self) -> impl Iterator<Item = &'a T> + '_ {
        self.range(0..self.len())
    }
}

impl<T, K> Index<usize> for View<'_, T, K> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        self.get(index).expect("shown row out of range")
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn a_page_that_starts_no_later_than_the_log_ends_restarts_it() {
        let mut log = Log::default();
        log.extend([1u64, 2, 3]);
        let behind = |last: &u64, first: &u64| first <= last;
        log.restart_if_behind(&4, behind);
        assert_eq!(log.len(), 3);
        log.restart_if_behind(&3, behind);
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn a_poller_reads_only_when_enabled_and_idle_and_waits_between_reads() {
        let now = Instant::now();
        let later = now + std::time::Duration::from_secs(1);
        let mut poller = Poller::default();
        assert!(!poller.due(false, 32, now));
        assert!(poller.due(true, 32, now));
        assert!(poller.start(32));
        assert!(!poller.due(true, 32, now));
        // a read of another client does not wait for this one
        assert!(poller.due(true, 44, now));
        assert!(poller.finish(32, Some(later)));
        assert!(!poller.due(true, 32, now));
        assert!(poller.due(true, 32, later));
        assert_eq!(poller.next_poll(), Some(later));
        assert!(!poller.start(32));
    }

    #[test]
    fn another_client_starts_over_and_the_one_left_is_not_heard() {
        let mut poller = Poller::default();
        assert!(poller.start(32));
        assert!(poller.start(44));
        assert!(!poller.finish(32, None));
        assert!(poller.finish(44, None));
    }

    fn shown(view: View<'_, i32, bool>) -> Vec<i32> {
        view.iter().copied().collect()
    }

    #[test]
    fn filters_only_new_rows_until_the_key_changes() {
        let mut log = Log::default();
        let filtered = Filtered::default();
        // shows the odd or the even rows, counting the rows it looks at
        let looked = Cell::new(0);
        let parity = |odd: bool| {
            let looked = &looked;
            move |n: &i32| {
                looked.set(looked.get() + 1);
                (n % 2 == 1) == odd
            }
        };
        log.extend(0..10);
        assert_eq!(
            shown(filtered.view(&log, false, parity(false))),
            [0, 2, 4, 6, 8]
        );
        assert_eq!(looked.replace(0), 10);
        assert_eq!(
            shown(filtered.view(&log, false, parity(false))),
            [0, 2, 4, 6, 8]
        );
        assert_eq!(looked.replace(0), 0);
        log.extend(10..13);
        assert_eq!(
            shown(filtered.view(&log, false, parity(false))),
            [0, 2, 4, 6, 8, 10, 12]
        );
        assert_eq!(looked.replace(0), 3);
        assert_eq!(
            shown(filtered.view(&log, true, parity(true))),
            [1, 3, 5, 7, 9, 11]
        );
        assert_eq!(looked.replace(0), 13);
    }

    #[test]
    fn drops_rows_that_leave_the_front() {
        let mut log = Log::default();
        let filtered = Filtered::default();
        log.extend(0..10);
        assert_eq!(filtered.view(&log, (), |_| true).len(), 10);
        log.keep(4);
        log.push(10);
        let view = filtered.view(&log, (), |_| true);
        assert_eq!(view.iter().copied().collect::<Vec<_>>(), [6, 7, 8, 9, 10]);
        assert_eq!((view[0], view.get(5)), (6, None));
        assert_eq!(view.range(1..3).copied().collect::<Vec<_>>(), [7, 8]);
        drop(view);
        log.clear();
        assert!(filtered.view(&log, (), |_| true).is_empty());
        log.push(11);
        assert_eq!(
            filtered
                .view(&log, (), |_| true)
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [11]
        );
    }

    #[test]
    fn a_fresh_log_is_filtered_from_scratch() {
        let filtered = Filtered::default();
        let mut log = Log::default();
        log.extend(0..5);
        assert_eq!(filtered.view(&log, (), |_| true).len(), 5);
        let mut fresh = Log::default();
        fresh.extend(0..2);
        assert_eq!(
            filtered
                .view(&fresh, (), |_| true)
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [0, 1]
        );
    }
}
