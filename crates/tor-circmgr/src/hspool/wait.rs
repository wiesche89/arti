//! Wait for one on-demand build or a suitable newly pooled circuit.

use crate::Result;
use event_listener::Event;
use futures::{Future, FutureExt};

/// Acquire exclusively from `take`, or finish the single fallback `build`.
///
/// `take` must apply all path restrictions on every call. Registering before
/// checking prevents an insertion between the check and the wait being lost.
pub(super) async fn take_or_build<T>(
    available: &Event,
    mut take: impl FnMut() -> Result<Option<T>>,
    build: impl Future<Output = Result<T>>,
) -> Result<T> {
    let build = build.fuse();
    futures::pin_mut!(build);
    loop {
        let changed = available.listen().fuse();
        futures::pin_mut!(changed);
        if let Some(circuit) = take()? {
            return Ok(circuit);
        }
        // Poll the same build throughout: notifications must not restart its
        // timeout or spawn another build. Errors retain the original semantics.
        futures::select_biased! {
            result = build => return result,
            _ = changed => {},
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Error;
    use futures::{FutureExt, channel::oneshot, executor::block_on, future, poll};
    use std::{
        cell::{Cell, RefCell},
        task::Poll,
    };

    #[test]
    fn notification_does_not_restart_sixty_second_build_timeout() {
        use std::time::Duration;
        use tor_rtcompat::SleepProvider;
        use tor_rtmock::MockRuntime;

        MockRuntime::test_with_various(|runtime| async move {
            let event = Event::new();
            let started = runtime.now();
            let build = async {
                runtime.sleep(Duration::from_secs(60)).await;
                Err::<usize, _>(Error::CircTimeout(None))
            };
            let acquire = take_or_build(&event, || Ok(None), build);
            futures::pin_mut!(acquire);
            assert!(poll!(&mut acquire).is_pending());
            runtime.advance_by(Duration::from_secs(30)).await;
            event.notify(usize::MAX);
            assert!(poll!(&mut acquire).is_pending());
            runtime.advance_by(Duration::from_secs(29)).await;
            assert!(poll!(&mut acquire).is_pending());
            runtime.advance_by(Duration::from_secs(1)).await;
            assert!(matches!(
                poll!(&mut acquire),
                Poll::Ready(Err(Error::CircTimeout(None)))
            ));
            assert_eq!(runtime.now() - started, Duration::from_secs(60));
        });
    }

    #[test]
    fn replenishment_overtakes_pending_build() {
        block_on(async {
            let event = Event::new();
            let pool = RefCell::new(Vec::new());
            let starts = Cell::new(0);
            let (tx, rx) = oneshot::channel::<usize>();
            let build = async {
                starts.set(starts.get() + 1);
                Ok(rx.await.unwrap())
            };
            let acquire = take_or_build(&event, || Ok(pool.borrow_mut().pop()), build);
            futures::pin_mut!(acquire);
            assert!(poll!(&mut acquire).is_pending());
            pool.borrow_mut().push(7);
            event.notify(usize::MAX);
            assert!(matches!(poll!(&mut acquire), Poll::Ready(Ok(7))));
            assert_eq!(starts.get(), 1);
            assert!(pool.borrow().is_empty());
            assert!(tx.send(42).is_err(), "the unused fallback is dropped");
        });
    }

    #[test]
    fn notification_between_check_and_wait_is_not_lost() {
        block_on(async {
            let event = Event::new();
            let checks = Cell::new(0);
            let acquire = take_or_build(
                &event,
                || {
                    checks.set(checks.get() + 1);
                    if checks.get() == 1 {
                        event.notify(usize::MAX);
                        Ok(None)
                    } else {
                        Ok(Some(9))
                    }
                },
                future::pending(),
            );
            futures::pin_mut!(acquire);
            assert!(matches!(poll!(&mut acquire), Poll::Ready(Ok(9))));
        });
    }

    #[test]
    fn rechecks_compatibility_and_leases_each_circuit_once() {
        block_on(async {
            let event = Event::new();
            let pool = RefCell::new(Vec::<usize>::new());
            let take = || {
                let mut pool = pool.borrow_mut();
                // Model a path restriction: only odd circuits are compatible.
                let at = pool.iter().position(|id| id % 2 == 1);
                Ok(at.map(|at| pool.remove(at)))
            };
            let a = take_or_build(&event, take, future::pending());
            let b = take_or_build(&event, take, future::pending());
            futures::pin_mut!(a, b);
            assert!(poll!(&mut a).is_pending());
            assert!(poll!(&mut b).is_pending());
            pool.borrow_mut().push(2);
            event.notify(usize::MAX);
            assert!(poll!(&mut a).is_pending());
            assert!(poll!(&mut b).is_pending());
            pool.borrow_mut().push(1);
            event.notify(usize::MAX);
            assert!(matches!(poll!(&mut a), Poll::Ready(Ok(1))));
            assert!(poll!(&mut b).is_pending());
            pool.borrow_mut().push(3);
            event.notify(usize::MAX);
            assert!(matches!(poll!(&mut b), Poll::Ready(Ok(3))));
            assert_eq!(*pool.borrow(), vec![2]);
        });
    }

    #[test]
    fn fallback_success_failure_and_cancellation() {
        block_on(async {
            let event = Event::new();
            assert_eq!(
                take_or_build(&event, || Ok(None), future::ready(Ok(5)))
                    .await
                    .unwrap(),
                5
            );
            assert!(matches!(
                take_or_build::<usize>(
                    &event,
                    || Ok(None),
                    future::ready(Err(Error::CircTimeout(None)))
                )
                .await,
                Err(Error::CircTimeout(None))
            ));
            assert_eq!(
                take_or_build(&event, || Ok(Some(8)), future::pending())
                    .await
                    .unwrap(),
                8
            );
            let mut pending =
                take_or_build::<usize>(&event, || Ok(None), future::pending()).boxed_local();
            assert!(poll!(&mut pending).is_pending());
            drop(pending);
            assert_eq!(event.notify(usize::MAX), 0);
        });
    }
}
