use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

use injector::{Item, ItemChange, ItemPage};

use crate::log_view::Poller;

pub const KEEP: usize = 25;
pub const INV_GAME_VAL: u32 = 2;
pub const WAIT_FOR_CHANGE: Duration = Duration::from_secs(1);
pub const RETRY_AFTER_ERROR: Duration = Duration::from_secs(1);

pub struct Poll {
    pub page: ItemPage,
    pub baseline: Option<Vec<(i32, Vec<Item>)>>,
    pub container_names: Vec<(i32, String)>,
    pub item_names: Vec<(i32, String)>,
    pub stackable: Vec<(i32, bool)>,
}

pub struct Request {
    pub since: Option<u64>,
    pub known_containers: Vec<i32>,
    pub known_items: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub tick: Option<i32>,
    pub items: Vec<Item>,
}

const MAX_COPIES: i64 = 4096;

pub type Delta = (i32, i32);

pub fn compare(
    previous: &[Item],
    current: &[Item],
    stackable: impl Fn(i32) -> bool,
) -> (Vec<Delta>, Vec<Delta>) {
    let mut quantities: BTreeMap<i32, i64> = BTreeMap::new();
    for item in previous.iter().filter(|item| item.id >= 0) {
        *quantities.entry(item.id).or_default() -= i64::from(item.quantity);
    }
    for item in current.iter().filter(|item| item.id >= 0) {
        *quantities.entry(item.id).or_default() += i64::from(item.quantity);
    }
    let (mut added, mut removed) = (Vec::new(), Vec::new());
    for (id, quantity) in quantities {
        let (list, copies) = if quantity > 0 {
            (&mut added, quantity)
        } else {
            (&mut removed, -quantity)
        };
        if copies == 0 {
            continue;
        }
        if stackable(id) {
            list.push((id, copies.min(i64::from(i32::MAX)) as i32));
        } else {
            list.extend(std::iter::repeat_n(
                (id, 1),
                copies.min(MAX_COPIES) as usize,
            ));
        }
    }
    (added, removed)
}

#[derive(Default)]
pub struct Inspector {
    pub enabled: bool,
    pub selected: Option<(i32, usize)>,
    pub error: Option<String>,
    logs: BTreeMap<i32, Vec<Snapshot>>,
    pending: Vec<(i32, Snapshot)>,
    copy: HashMap<i32, Vec<Item>>,
    next: Option<u64>,
    container_names: HashMap<i32, String>,
    item_names: HashMap<i32, String>,
    stackable: HashMap<i32, bool>,
    poller: Poller,
}

impl Inspector {
    pub fn logs(&self) -> &BTreeMap<i32, Vec<Snapshot>> {
        &self.logs
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    pub fn container_label(&self, id: i32) -> String {
        match self.container_names.get(&id) {
            Some(name) => format!("{id} - {name}"),
            None => id.to_string(),
        }
    }

    pub fn item_name(&self, id: i32) -> &str {
        self.item_names.get(&id).map_or("?", String::as_str)
    }

    pub fn is_stackable(&self, id: i32) -> bool {
        self.stackable.get(&id).copied().unwrap_or(true)
    }

    pub fn clear(&mut self) {
        self.logs.clear();
        self.pending.clear();
        self.selected = None;
    }

    pub fn refresh(&mut self) {
        self.selected = None;
        for (id, snapshot) in std::mem::take(&mut self.pending) {
            let log = self.logs.entry(id).or_default();
            log.push(snapshot);
            if log.len() > KEEP {
                log.remove(0);
            }
        }
    }

    pub fn picked(&self) -> Option<(&Snapshot, Option<&Snapshot>)> {
        let (id, index) = self.selected?;
        let log = self.logs.get(&id)?;
        Some((
            log.get(index)?,
            index.checked_sub(1).and_then(|before| log.get(before)),
        ))
    }

    pub fn retry_in(&self, now: Instant) -> Option<Duration> {
        self.poller
            .next_poll()
            .map(|at| at.saturating_duration_since(now))
    }

    pub fn due(&self, pid: u32, now: Instant) -> bool {
        self.poller.due(self.enabled, pid, now)
    }

    pub fn start(&mut self, pid: u32) -> Request {
        if self.poller.start(pid) {
            self.clear();
            self.copy.clear();
            self.next = None;
        }
        Request {
            since: self.next,
            known_containers: self.container_names.keys().copied().collect(),
            known_items: self.item_names.keys().copied().collect(),
        }
    }

    pub fn finish(&mut self, pid: u32, now: Instant, result: Result<Poll, String>) {
        if !self
            .poller
            .finish(pid, result.is_err().then_some(now + RETRY_AFTER_ERROR))
        {
            return;
        }
        match result {
            Ok(Poll {
                page,
                baseline,
                container_names,
                item_names,
                stackable,
            }) => {
                self.error = None;
                self.container_names.extend(container_names);
                self.item_names.extend(item_names);
                self.stackable.extend(stackable);
                if let Some(baseline) = baseline {
                    self.copy = baseline.into_iter().collect();
                    self.next = Some(page.next);
                    return;
                }
                let expected = self.next.unwrap_or(0);
                if page.dropped > 0
                    || page
                        .changes
                        .first()
                        .is_some_and(|first| first.sequence != expected)
                {
                    self.next = None;
                    return;
                }
                self.next = Some(page.next);
                self.apply(&page.changes);
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn apply(&mut self, changes: &[ItemChange]) {
        let mut run: Option<(i32, Option<i32>)> = None;
        for change in changes {
            let key = (change.container, change.tick);
            if run.is_some_and(|run| run != key) {
                self.snapshot(run.take().unwrap());
            }
            run = Some(key);
            let items = self.copy.entry(change.container).or_default();
            if !change.old_known {
                items.clear();
            }
            let Ok(slot) = usize::try_from(change.slot) else {
                continue;
            };
            while items.len() <= slot {
                items.push(Item {
                    slot: items.len() as i32,
                    id: -1,
                    quantity: 0,
                });
            }
            items[slot] = Item {
                slot: change.slot,
                id: change.new_item,
                quantity: change.new_quantity,
            };
        }
        if let Some(run) = run {
            self.snapshot(run);
        }
    }

    fn snapshot(&mut self, (container, tick): (i32, Option<i32>)) {
        if let Some(items) = self.copy.get(&container) {
            self.pending.push((
                container,
                Snapshot {
                    tick,
                    items: items.clone(),
                },
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(slot: i32, id: i32, quantity: i32) -> Item {
        Item { slot, id, quantity }
    }

    #[test]
    fn compares_by_item_and_ignores_slot_moves() {
        let before = [item(0, 995, 100), item(1, 1511, 1), item(2, -1, 0)];
        let after = [item(0, 1511, 1), item(1, 995, 250), item(2, 590, 1)];
        let stacks = |id| id == 995;
        assert_eq!(
            compare(&before, &after, stacks),
            (vec![(590, 1), (995, 150)], vec![])
        );
        assert_eq!(
            compare(&after, &before, stacks),
            (vec![], vec![(590, 1), (995, 150)])
        );
        assert_eq!(compare(&before, &before, stacks), (vec![], vec![]));
    }

    #[test]
    fn lists_each_copy_of_an_item_that_does_not_stack() {
        let after = [item(0, 1511, 1), item(1, 1511, 1), item(2, 995, 3)];
        assert_eq!(
            compare(&[], &after, |id| id == 995),
            (vec![(995, 3), (1511, 1), (1511, 1)], vec![])
        );
        assert_eq!(
            compare(&after, &[], |id| id == 995),
            (vec![], vec![(995, 3), (1511, 1), (1511, 1)])
        );
    }

    #[test]
    fn compares_containers_of_different_size() {
        assert_eq!(
            compare(&[item(0, 1, 1)], &[item(0, 1, 1), item(1, 2, 3)], |_| true),
            (vec![(2, 3)], vec![])
        );
    }

    fn change(
        sequence: u64,
        container: i32,
        tick: i32,
        slot: i32,
        old_known: bool,
        new_item: i32,
        new_quantity: i32,
    ) -> ItemChange {
        ItemChange {
            sequence,
            time_ms: 0,
            tick: Some(tick),
            container,
            slot,
            old_known,
            old_item: -1,
            old_quantity: 0,
            new_item,
            new_quantity,
        }
    }

    fn poll(
        next: u64,
        baseline: Option<Vec<(i32, Vec<Item>)>>,
        changes: Vec<ItemChange>,
    ) -> Result<Poll, String> {
        Ok(Poll {
            page: ItemPage {
                changes,
                next,
                dropped: 0,
            },
            baseline,
            container_names: vec![(93, "inv".into())],
            item_names: vec![],
            stackable: vec![(1511, false)],
        })
    }

    fn started() -> Inspector {
        let mut inspector = Inspector {
            enabled: true,
            ..Inspector::default()
        };
        assert_eq!(inspector.start(1).since, None);
        inspector.finish(
            1,
            Instant::now(),
            poll(
                10,
                Some(vec![(93, vec![item(0, 995, 5), item(1, -1, 0)])]),
                vec![],
            ),
        );
        inspector
    }

    fn read(inspector: &mut Inspector, next: u64, changes: Vec<ItemChange>) {
        inspector.start(1);
        inspector.finish(1, Instant::now(), poll(next, None, changes));
    }

    #[test]
    fn the_baseline_is_not_a_snapshot_and_changes_wait_for_refresh() {
        let mut inspector = started();
        assert_eq!(inspector.pending(), 0);
        assert_eq!(inspector.start(1).since, Some(10));
        inspector.finish(
            1,
            Instant::now(),
            poll(
                12,
                None,
                vec![
                    change(10, 93, 7, 1, true, 1511, 1),
                    change(11, 93, 7, 0, true, 995, 9),
                ],
            ),
        );
        assert_eq!(inspector.pending(), 1);
        assert!(inspector.logs().is_empty());
        inspector.refresh();
        let log = &inspector.logs()[&93];
        assert_eq!(log[0].tick, Some(7));
        assert_eq!(log[0].items, [item(0, 995, 9), item(1, 1511, 1)]);
        assert_eq!(inspector.container_label(93), "93 - inv");
        assert_eq!(inspector.container_label(94), "94");
    }

    #[test]
    fn each_tick_and_container_is_its_own_snapshot() {
        let mut inspector = started();
        read(
            &mut inspector,
            13,
            vec![
                change(10, 93, 7, 1, true, 1511, 1),
                change(11, 93, 8, 1, true, 590, 1),
                change(12, 94, 8, 0, false, 4151, 1),
            ],
        );
        inspector.refresh();
        assert_eq!(inspector.logs()[&93].len(), 2);
        assert_eq!(inspector.logs()[&93][1].items[1], item(1, 590, 1));
        assert_eq!(inspector.logs()[&94][0].items, [item(0, 4151, 1)]);
        inspector.selected = Some((93, 1));
        let (shown, before) = inspector.picked().unwrap();
        assert_eq!(shown.items[1].id, 590);
        assert_eq!(before.unwrap().items[1].id, 1511);
    }

    #[test]
    fn a_write_that_makes_the_container_starts_it_empty() {
        let mut inspector = started();
        read(
            &mut inspector,
            11,
            vec![change(10, 93, 7, 3, false, 1511, 1)],
        );
        inspector.refresh();
        assert_eq!(
            inspector.logs()[&93][0].items,
            [
                item(0, -1, 0),
                item(1, -1, 0),
                item(2, -1, 0),
                item(3, 1511, 1)
            ]
        );
    }

    #[test]
    fn a_log_that_skipped_changes_is_read_again_from_a_baseline() {
        let mut inspector = started();
        read(&mut inspector, 21, vec![change(20, 93, 7, 0, true, 1, 1)]);
        assert_eq!(inspector.pending(), 0);
        assert_eq!(inspector.start(1).since, None);
        inspector.finish(
            1,
            Instant::now(),
            poll(30, Some(vec![(93, vec![item(0, 1, 1)])]), vec![]),
        );
        assert_eq!(inspector.start(1).since, Some(30));
        inspector.finish(
            1,
            Instant::now(),
            Ok(Poll {
                page: ItemPage {
                    changes: vec![],
                    next: 40,
                    dropped: 3,
                },
                baseline: None,
                container_names: vec![],
                item_names: vec![],
                stackable: vec![],
            }),
        );
        assert_eq!(inspector.start(1).since, None);
    }

    #[test]
    fn keeps_the_newest_snapshots() {
        let mut inspector = started();
        for quantity in 0..(KEEP as i32 + 5) {
            let sequence = 10 + quantity as u64;
            read(
                &mut inspector,
                sequence + 1,
                vec![change(sequence, 93, quantity, 0, true, 995, quantity)],
            );
            inspector.refresh();
        }
        let log = &inspector.logs()[&93];
        assert_eq!(log.len(), KEEP);
        assert_eq!(log[0].items[0].quantity, 5);
    }
}
