//! Admission to, and dispatch onto, the big-stack documentation pool.
//!
//! Every documentation handler — `/api`, `/mcp`, the og cards — funnels its
//! synchronous work onto one small [`rayon`] pool (see the [module
//! docs](super)). Rayon's queue is first-come-first-served and unbounded, which
//! fails badly under overload: once arrivals outrun the workers the queue grows
//! without limit, every job waits behind all the earlier ones, and clients give
//! up long before theirs reaches the front. Each abandoned job still runs to
//! completion and writes to a dead socket, so the workers stay fully busy while
//! goodput falls to zero. On a burstable instance this needs no unusual load to
//! trigger — draining the CPU credits cuts capacity several-fold, and arrivals
//! that were comfortably served become an ever-growing backlog.
//!
//! Two mechanisms here keep the queue short and the work live:
//!
//! - **Cancellation.** The whole wait — for admission, then for the worker — runs under
//!   [`Conn::cancel_on_disconnect`], so a client that leaves stops waiting at once, freeing its
//!   connection. A job already handed to rayon cannot be recalled, so it checks at start whether
//!   anyone is still listening and skips the work if not.
//! - **Admission.** Each [`Lane`] holds as many permits as the pool has threads, and a job needs
//!   one to be handed to rayon at all, so rayon's own queue never holds more than a lane's worth
//!   per lane. A job that cannot get a permit within the lane's wait budget is refused with a `503`
//!   rather than queued behind work it will never outlive. The bound is on *time*, not on a count
//!   tuned to one capacity: whatever the CPU is currently delivering, no admitted job has waited
//!   longer than the budget.
//!
//! Lanes are admitted independently so one traffic class saturating its share
//! cannot starve the others — a scraper flooding `/api` must not queue an agent's
//! `/mcp` call behind it.

use async_lock::Semaphore;
use rayon::ThreadPool;
use std::{
    fmt::{self, Display, Formatter},
    panic::{self, AssertUnwindSafe},
    sync::Arc,
    time::Duration,
};
use trillium::{Conn, KnownHeaderName, Status};
use trillium_conn_id::ConnIdExt;
use trillium_logger::formatters;
use trillium_smol::SmolRuntime;

/// The `Retry-After` sent with an overload `503`, in seconds.
const RETRY_AFTER_SECONDS: &str = "5";

/// A traffic class sharing the documentation pool, admitted separately from the
/// others.
#[derive(Clone, Copy, Debug)]
pub(super) enum Lane {
    /// The JSON API behind the web client.
    Api,
    /// The MCP endpoint.
    Mcp,
    /// The og card renderer.
    Og,
}

impl Lane {
    /// How long a job may wait for a permit before it is refused.
    ///
    /// The browser-facing lanes are short: a person is waiting on `/api`, and
    /// link unfurlers give up on og cards within seconds. Agents calling `/mcp`
    /// tolerate a longer wait, and its traffic is too light to build a backlog
    /// that would make one.
    fn max_wait(self) -> Duration {
        match self {
            Self::Api | Self::Og => Duration::from_secs(3),
            Self::Mcp => Duration::from_secs(10),
        }
    }
}

/// Marks a conn whose client disconnected before its job finished, so the
/// access log can say so (see [`logged_status`]). Nothing is sent: nobody is
/// listening.
struct Abandoned;

/// The access log's status field: `499` for a request its client
/// [`Abandoned`] (nginx's "Client Closed Request"), otherwise the status sent.
///
/// Without this an abandoned request logs as `404` — trillium's default for a
/// halted conn with no status — indistinguishable from a real miss, and from
/// the `503` of a request we refused.
pub(super) fn logged_status(
    conn: &Conn,
    color: bool,
) -> LoggedStatus<impl Display + Send + Sync + use<>> {
    if conn.state::<Abandoned>().is_some() {
        LoggedStatus::Abandoned { color }
    } else {
        LoggedStatus::Sent(formatters::status(conn, color))
    }
}

/// See [`logged_status`].
pub(super) enum LoggedStatus<S> {
    Abandoned { color: bool },
    Sent(S),
}

impl<S: Display> Display for LoggedStatus<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            // Yellow, as trillium-logger colors every other 4xx.
            Self::Abandoned { color: true } => f.write_str("\x1b[33m499\x1b[0m"),
            Self::Abandoned { color: false } => f.write_str("499"),
            Self::Sent(status) => status.fmt(f),
        }
    }
}

/// Why a job produced no result.
pub(super) enum Refusal {
    /// The client went away before the job finished; nobody is listening.
    Disconnected,
    /// No permit became free within the lane's wait budget.
    Overloaded,
    /// The job panicked.
    Panicked,
}

impl Refusal {
    /// The response for a job that produced no result. Every arm halts, so the
    /// trailing frontend handler doesn't answer with the SPA index instead.
    pub(super) fn respond(self, conn: Conn) -> Conn {
        match self {
            Self::Disconnected => conn.halt(),
            Self::Overloaded => conn
                .with_status(Status::ServiceUnavailable)
                .with_response_header(KnownHeaderName::RetryAfter, RETRY_AFTER_SECONDS)
                .halt(),
            Self::Panicked => conn.with_status(Status::InternalServerError).halt(),
        }
    }
}

/// The big-stack pool and the per-[`Lane`] permits that admit work onto it.
pub(super) struct Workers {
    pool: ThreadPool,
    api: Arc<Semaphore>,
    mcp: Arc<Semaphore>,
    og: Arc<Semaphore>,
}

impl Workers {
    /// Give each lane one permit per pool thread: enough for any one lane to
    /// occupy the whole pool when the others are idle.
    pub(super) fn new(pool: ThreadPool) -> Self {
        let permits = pool.current_num_threads();
        let lane = || Arc::new(Semaphore::new(permits));
        Self {
            pool,
            api: lane(),
            mcp: lane(),
            og: lane(),
        }
    }

    fn permits(&self, lane: Lane) -> &Arc<Semaphore> {
        match lane {
            Lane::Api => &self.api,
            Lane::Mcp => &self.mcp,
            Lane::Og => &self.og,
        }
    }

    /// Run a synchronous, stack-hungry job on a pool worker and await its owned
    /// result, abandoning the wait if `conn`'s client disconnects.
    ///
    /// On HTTP/1.x, disconnect detection reads the transport while the job is
    /// pending, so a request body must already have been read.
    pub(super) async fn run<F, T>(&self, conn: &mut Conn, lane: Lane, job: F) -> Result<T, Refusal>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        match conn.cancel_on_disconnect(self.dispatch(lane, job)).await {
            Some(Err(Refusal::Overloaded)) => {
                // Its own line rather than a field on the access line, whose
                // layout log tooling parses; the conn id joins the two.
                log::info!(
                    "[{}] shed {lane:?}: no worker within {}s",
                    conn.id(),
                    lane.max_wait().as_secs()
                );
                Err(Refusal::Overloaded)
            }
            Some(outcome) => outcome,
            None => {
                log::info!("[{}] client disconnected awaiting {lane:?} job", conn.id());
                conn.insert_state(Abandoned);
                Err(Refusal::Disconnected)
            }
        }
    }

    /// Wait for a permit, then hand the job to rayon and wait for its result.
    async fn dispatch<F, T>(&self, lane: Lane, job: F) -> Result<T, Refusal>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let permit = SmolRuntime::default()
            .timeout(lane.max_wait(), self.permits(lane).acquire_arc())
            .await
            .ok_or(Refusal::Overloaded)?;

        let (tx, rx) = async_channel::bounded(1);
        self.pool.spawn(move || {
            // Held until the job is done with the worker, not merely dequeued.
            let _permit = permit;
            // The receiver is dropped when the client disconnects while this job
            // sits in rayon's queue; nothing would read the result.
            if tx.is_closed() {
                return;
            }
            let outcome = panic::catch_unwind(AssertUnwindSafe(job)).ok();
            let _ = tx.send_blocking(outcome);
        });

        rx.recv().await.ok().flatten().ok_or(Refusal::Panicked)
    }
}
