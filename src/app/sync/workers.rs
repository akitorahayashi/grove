use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

pub(super) fn map<T, R>(items: &[T], parallelism: usize, action: impl Fn(&T) -> R + Sync) -> Vec<R>
where
    T: Sync,
    R: Send,
{
    map_indexed(items, parallelism, |_, item| action(item))
}

pub(super) fn map_keyed<T, R, K>(
    items: &[T],
    parallelism: usize,
    key: impl Fn(&T) -> K,
    action: impl Fn(&T) -> R + Sync,
) -> Vec<R>
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
    });
    let mut ordered = std::iter::repeat_with(|| None).take(items.len()).collect::<Vec<_>>();
    for (index, result) in grouped.into_iter().flatten() {
        ordered[index] = Some(result);
    }
    ordered
        .into_iter()
        .map(|result| result.expect("every keyed phase result slot should be filled"))
        .collect()
}

fn map_indexed<T, R>(
    items: &[T],
    parallelism: usize,
    action: impl Fn(usize, &T) -> R + Sync,
) -> Vec<R>
where
    T: Sync,
    R: Send,
{
    if items.is_empty() {
        return Vec::new();
    }

    let worker_count = parallelism.min(items.len());
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
                    let result = action(index, item);
                    sender.send((index, result)).expect("phase result receiver should remain open");
                }
            });
        }
    });

    let mut ordered = std::iter::repeat_with(|| None).take(items.len()).collect::<Vec<_>>();
    for _ in 0..items.len() {
        let (index, result) = receiver.recv().expect("every phase task should produce a result");
        ordered[index] = Some(result);
    }

    ordered
        .into_iter()
        .map(|result| result.expect("every phase result slot should be filled"))
        .collect()
}
