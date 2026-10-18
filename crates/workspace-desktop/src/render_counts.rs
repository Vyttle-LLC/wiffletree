//! Development-only render counts. With `WIFFLETREE_RENDER_COUNTS` set, each view's renders
//! per second go to stderr about once a second while anything renders.
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::LazyLock,
    time::{Duration, Instant},
};

static ENABLED: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("WIFFLETREE_RENDER_COUNTS").is_some());

thread_local! {
    static COUNTS: RefCell<(Instant, BTreeMap<&'static str, u32>)> =
        RefCell::new((Instant::now(), BTreeMap::new()));
}

/// Counts one render of `view`.
pub(super) fn count(view: &'static str) {
    if !*ENABLED {
        return;
    }
    COUNTS.with_borrow_mut(|(since, counts)| {
        *counts.entry(view).or_default() += 1;
        let elapsed = since.elapsed();
        if elapsed >= Duration::from_secs(1) {
            let rates: Vec<String> = (counts.iter())
                .map(|(view, n)| format!("{view}={:.1}", *n as f64 / elapsed.as_secs_f64()))
                .collect();
            eprintln!(
                "renders/s over {:.1}s: {}",
                elapsed.as_secs_f64(),
                rates.join(" ")
            );
            counts.clear();
            *since = Instant::now();
        }
    });
}
