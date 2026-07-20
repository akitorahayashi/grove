use std::collections::HashMap;
use std::hash::Hash;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use crate::AppError;

const MAX_WORKERS: usize = 8;

pub(crate) fn map<T, R>(
    items: &[T],
    parallelism: usize,
    action: impl Fn(&T) -> R + Sync,
) -> Result<Vec<R>, AppError>
where
    T: Sync,
    R: Send,
{
    map_indexed(items, parallelism, |_, item| action(item))
}

pub(crate) fn map_keyed<T, R, K>(
    items: &[T],
    parallelism: usize,
    key: impl Fn(&T) -> K,
    action: impl Fn(&T) -> R + Sync,
) -> Result<Vec<R>, AppError>
where
    T: Sync,
    R: Send,
    K: Eq + Hash,
{
    let mut group_by_key = HashMap::new();
    let mut groups = Vec::<Vec<usize>>::new();
    for (index, item) in items.iter().enumerate() {
        let group = *group_by_key.entry(key(item)).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group].push(index);
    }

    let grouped = map(&groups, parallelism, |group| {
        group.iter().map(|&index| (index, action(&items[index]))).collect::<Vec<_>>()
    })?;
    let mut ordered = std::iter::repeat_with(|| None).take(items.len()).collect::<Vec<_>>();
    for (index, result) in grouped.into_iter().flatten() {
        ordered[index] = Some(result);
    }
    ordered
        .into_iter()
        .map(|result| result.ok_or_else(|| AppError::internal("keyed worker omitted a result")))
        .collect()
}

fn map_indexed<T, R>(
    items: &[T],
    parallelism: usize,
    action: impl Fn(usize, &T) -> R + Sync,
) -> Result<Vec<R>, AppError>
where
    T: Sync,
    R: Send,
{
    if items.is_empty() {
        return Ok(Vec::new());
    }

    let worker_count = parallelism.max(1).min(items.len()).min(MAX_WORKERS);
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let sender = sender.clone();
            let action = &action;
            let next = &next;

            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    let result = catch_unwind(AssertUnwindSafe(|| action(index, item)))
                        .map_err(|_| AppError::internal("repository worker panicked"));
                    if sender.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
    });
    drop(sender);

    let mut ordered = std::iter::repeat_with(|| None).take(items.len()).collect::<Vec<_>>();
    for _ in 0..items.len() {
        let (index, result) = receiver
            .recv()
            .map_err(|_| AppError::internal("repository worker result channel disconnected"))?;
        ordered[index] = Some(result?);
    }

    ordered
        .into_iter()
        .map(|result| {
            result.ok_or_else(|| AppError::internal("repository worker omitted a result"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    use super::{map, map_keyed};

    #[test]
    fn preserves_order_and_handles_zero_items() {
        assert_eq!(map(&[3, 1, 2], 8, |value| value * 2).unwrap(), [6, 2, 4]);
        assert!(map::<u8, u8>(&[], 8, |value| *value).unwrap().is_empty());
    }

    #[test]
    fn limits_live_work_to_eight_tasks() {
        let barrier = Barrier::new(8);
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let items = (0..16).collect::<Vec<_>>();

        map(&items, 64, |_| {
            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            barrier.wait();
            active.fetch_sub(1, Ordering::SeqCst);
        })
        .unwrap();

        assert_eq!(peak.load(Ordering::SeqCst), 8);
    }

    #[test]
    fn serializes_shared_keys_while_independent_keys_overlap() {
        let barrier = Arc::new(Barrier::new(2));
        let active_a = AtomicUsize::new(0);
        let active_b = AtomicUsize::new(0);

        map_keyed(
            &[('a', 1), ('a', 2), ('b', 3), ('b', 4)],
            8,
            |item| item.0,
            |item| {
                let active = if item.0 == 'a' { &active_a } else { &active_b };
                assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                if item.1 == 1 || item.1 == 3 {
                    barrier.wait();
                }
                active.fetch_sub(1, Ordering::SeqCst);
                item.1
            },
        )
        .unwrap();
    }

    #[test]
    fn converts_action_panic_to_application_error() {
        let result = map(&[1], 1, |_| panic!("expected panic"));

        assert!(result.is_err_and(|error| error.to_string().contains("worker panicked")));
    }
}
