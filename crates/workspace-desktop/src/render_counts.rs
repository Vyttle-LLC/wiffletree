//! Development-only render counts and main-thread timings. With `WIFFLETREE_RENDER_COUNTS` set,
//! each view's renders per second go to stderr about once a second while anything renders, and
//! once a second a line with the timed stages, how often each watched view was notified, and the
//! longest main-thread stall. Notifies do not depend on whether the window is drawn.
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::LazyLock,
    time::{Duration, Instant},
};

static ENABLED: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("WIFFLETREE_RENDER_COUNTS").is_some());

/// How often the stall watch asks to run; how much later it runs is how long the thread was busy.
const STALL_TICK: Duration = Duration::from_millis(5);
/// A stall longer than this misses a 60 Hz frame.
const FRAME: Duration = Duration::from_micros(16_667);

thread_local! {
    static COUNTS: RefCell<(Instant, BTreeMap<&'static str, u32>)> =
        RefCell::new((Instant::now(), BTreeMap::new()));
    static STAGES: RefCell<BTreeMap<&'static str, Stage>> = RefCell::default();
    static NOTIFIES: RefCell<BTreeMap<&'static str, u32>> = RefCell::default();
}

#[derive(Default)]
struct Stage {
    count: u32,
    total: Duration,
    longest: Duration,
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

pub(super) fn timed<R>(stage: &'static str, work: impl FnOnce() -> R) -> R {
    if !*ENABLED {
        return work();
    }
    let started = Instant::now();
    let result = work();
    record(stage, started.elapsed());
    result
}

fn record(stage: &'static str, took: Duration) {
    STAGES.with_borrow_mut(|stages| {
        let stage = stages.entry(stage).or_default();
        stage.count += 1;
        stage.total += took;
        stage.longest = stage.longest.max(took);
    });
}

pub(super) fn count_notifies<T: 'static, V: 'static>(
    view: &gpui::Entity<V>,
    name: &'static str,
    cx: &mut gpui::Context<T>,
) {
    if *ENABLED {
        cx.observe(view, move |_, _, _| {
            NOTIFIES.with_borrow_mut(|notifies| *notifies.entry(name).or_default() += 1)
        })
        .detach();
    }
}

/// Watches the main thread for stalls and prints the timed stages once a second.
pub(super) fn watch_stalls(cx: &mut gpui::App) {
    if !*ENABLED {
        return;
    }
    cx.spawn(async move |cx| {
        let mut since = Instant::now();
        let mut cpu_since = thread_cpu();
        loop {
            let asked = Instant::now();
            cx.background_executor().timer(STALL_TICK).await;
            let stall = asked.elapsed().saturating_sub(STALL_TICK);
            record("stall", stall);
            if stall > FRAME {
                record("missed frame", stall);
            }
            if since.elapsed() >= Duration::from_secs(1) {
                let cpu = thread_cpu();
                print_stages(since.elapsed(), cpu - cpu_since);
                (since, cpu_since) = (Instant::now(), cpu);
            }
        }
    })
    .detach();
}

fn thread_cpu() -> Duration {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `time` is a valid timespec to write to.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) };
    Duration::new(time.tv_sec as u64, time.tv_nsec as u32)
}

fn print_stages(elapsed: Duration, cpu: Duration) {
    let ms = |d: Duration| d.as_secs_f64() * 1000.;
    let lines: Vec<String> = STAGES.with_borrow_mut(|stages| {
        let lines = stages.iter().map(|(name, stage)| match *name {
            "stall" => format!("longest stall={:.1}ms", ms(stage.longest)),
            "missed frame" => format!("missed frames={}", stage.count),
            _ => format!(
                "{name}={:.1}/s avg={:.2}ms max={:.2}ms",
                stage.count as f64 / elapsed.as_secs_f64(),
                ms(stage.total) / stage.count as f64,
                ms(stage.longest)
            ),
        });
        let mut lines: Vec<String> = lines.collect();
        lines.push(format!(
            "main cpu={:.0}ms/s",
            ms(cpu) / elapsed.as_secs_f64()
        ));
        stages.clear();
        NOTIFIES.with_borrow_mut(|notifies| {
            let rates = (notifies.iter())
                .map(|(view, n)| format!("{view}={:.1}", *n as f64 / elapsed.as_secs_f64()));
            lines.push(format!(
                "notifies/s {}",
                rates.collect::<Vec<_>>().join(" ")
            ));
            notifies.clear();
        });
        lines
    });
    eprintln!(
        "main thread over {:.1}s: {}",
        elapsed.as_secs_f64(),
        lines.join(" | ")
    );
}
