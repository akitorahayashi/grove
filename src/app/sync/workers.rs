use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

pub(super) fn map<T, R>(items: &[T], parallelism: usize, action: impl Fn(&T) -> R + Sync) -> Vec<R>
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
                    let result = action(item);
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
