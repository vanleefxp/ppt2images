//! Bounded worker execution for page-level parallelism.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use anyhow::{Result, bail};

/// Automatic mode uses at most this many workers.
pub const AUTO_JOBS_LIMIT: usize = 8;

/// Resolves the number of workers to use for `tasks` independent items.
pub fn worker_count(requested: usize, tasks: usize) -> usize {
    if tasks <= 1 || requested == 1 {
        return 1;
    }
    let workers = if requested == 0 {
        thread::available_parallelism()
            .map_or(1, |count| count.get())
            .min(AUTO_JOBS_LIMIT)
    } else {
        requested
    };
    workers.clamp(1, tasks)
}

/// Runs `action` for every index in `0..tasks` using at most `requested` workers.
pub fn for_each<F>(requested: usize, tasks: usize, action: F) -> Result<()>
where
    F: Fn(usize) -> Result<()> + Sync,
{
    for_each_with(requested, tasks, || Ok(()), |(), index| action(index))
}

/// Runs `action` for every index in `0..tasks`, with one reusable state value
/// per worker created by `init`.
pub fn for_each_with<S, I, F>(requested: usize, tasks: usize, init: I, action: F) -> Result<()>
where
    I: Fn() -> Result<S> + Sync,
    F: Fn(&mut S, usize) -> Result<()> + Sync,
{
    if tasks == 0 {
        return Ok(());
    }
    let workers = worker_count(requested, tasks);
    if workers == 1 {
        let mut state = init()?;
        for index in 0..tasks {
            action(&mut state, index)?;
        }
        return Ok(());
    }

    let next = AtomicUsize::new(0);
    let error = Mutex::new(None);
    thread::scope(|scope| {
        for worker in 0..workers {
            let error = &error;
            let next = &next;
            let init = &init;
            let action = &action;
            let spawned = thread::Builder::new()
                .name(format!("ppt2images-worker-{worker}"))
                .spawn_scoped(scope, move || {
                    worker_loop(error, next, init, action, tasks);
                });
            if let Err(spawn_error) = spawned {
                record_error(
                    error,
                    anyhow::anyhow!("failed to start a parallel worker: {spawn_error}"),
                );
                break;
            }
        }
    });

    match error.into_inner() {
        Ok(Some(error)) => Err(error),
        Ok(None) => Ok(()),
        Err(_) => bail!("parallel worker coordination failed"),
    }
}

fn worker_loop<S, I, F>(
    error: &Mutex<Option<anyhow::Error>>,
    next: &AtomicUsize,
    init: &I,
    action: &F,
    tasks: usize,
) where
    I: Fn() -> Result<S> + Sync,
    F: Fn(&mut S, usize) -> Result<()> + Sync,
{
    let mut state = match init() {
        Ok(state) => state,
        Err(error_value) => {
            record_error(error, error_value);
            return;
        }
    };
    loop {
        if has_error(error) {
            break;
        }
        let index = next.fetch_add(1, Ordering::Relaxed);
        if index >= tasks {
            break;
        }
        if let Err(error_value) = action(&mut state, index) {
            record_error(error, error_value);
            break;
        }
    }
}

fn has_error(error: &Mutex<Option<anyhow::Error>>) -> bool {
    error.lock().map_or(true, |slot| slot.is_some())
}

fn record_error(error: &Mutex<Option<anyhow::Error>>, value: anyhow::Error) {
    if let Ok(mut slot) = error.lock()
        && slot.is_none()
    {
        *slot = Some(value);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{for_each, for_each_with, worker_count};

    #[test]
    fn resolves_worker_counts() {
        assert_eq!(worker_count(1, 10), 1);
        assert_eq!(worker_count(4, 2), 2);
        assert!((1..=super::AUTO_JOBS_LIMIT).contains(&worker_count(0, 100)));
        assert_eq!(worker_count(0, 1), 1);
    }

    #[test]
    fn visits_every_index_once() {
        let total = AtomicUsize::new(0);
        for_each(4, 100, |index| {
            total.fetch_add(index + 1, Ordering::Relaxed);
            Ok(())
        })
        .unwrap();
        assert_eq!(total.load(Ordering::Relaxed), 5050);
    }

    #[test]
    fn creates_one_reusable_state_per_worker() {
        let initializations = AtomicUsize::new(0);
        let visited = AtomicUsize::new(0);
        for_each_with(
            4,
            100,
            || {
                initializations.fetch_add(1, Ordering::Relaxed);
                Ok(0usize)
            },
            |state, _index| {
                *state += 1;
                visited.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(visited.load(Ordering::Relaxed), 100);
        assert!((1..=4).contains(&initializations.load(Ordering::Relaxed)));
    }

    #[test]
    fn propagates_worker_errors() {
        let error = for_each(4, 100, |index| {
            if index == 7 {
                anyhow::bail!("worker failed");
            }
            Ok(())
        })
        .unwrap_err();
        assert!(error.to_string().contains("worker failed"));
    }
}
