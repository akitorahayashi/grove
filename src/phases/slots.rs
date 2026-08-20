use crate::AppError;

/// Selection-ordered result slots for phase pipelines. Each selected item must
/// end up with exactly one result; a hole at completion is an internal error.
/// Owning that invariant here keeps the workers and the use cases that fill
/// entries across several phases from restating it.
pub(crate) struct Slots<T> {
    slots: Vec<Option<T>>,
}

impl<T> Slots<T> {
    pub(crate) fn new(len: usize) -> Self {
        Self { slots: std::iter::repeat_with(|| None).take(len).collect() }
    }

    pub(crate) fn fill(&mut self, index: usize, value: T) {
        debug_assert!(self.slots[index].is_none(), "slot {index} was already filled");
        self.slots[index] = Some(value);
    }

    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.slots[index].as_mut()
    }

    pub(crate) fn into_complete(self) -> Result<Vec<T>, AppError> {
        self.slots
            .into_iter()
            .map(|slot| slot.ok_or_else(|| AppError::internal("a work item produced no result")))
            .collect()
    }
}
