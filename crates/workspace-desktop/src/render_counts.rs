//! Development-only render counts and main-thread timings. With `WIFFLETREE_RENDER_COUNTS` set,
//! each view's renders per second go to stderr about once a second while anything renders, and
//! once a second a line with the timed stages, how often each watched view was notified, and the
//! longest main-thread stall. Each stall that misses a frame gets a line of its own, with the
//! renders and timed stages that ran during it: its likely cause. Notifies do not
//! depend on whether the window is drawn.
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::LazyLock,
    time::{Duration, Instant},
};

static ENABLED: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("WIFFLETREE_RENDER_COUNTS").is_some());

/// A stall longer than this misses a 60 Hz frame.
const FRAME: Duration = Duration::from_micros(16_667);

thread_local! {
    static COUNTS: RefCell<(Instant, BTreeMap<&'static str, u32>)> =
        RefCell::new((Instant::now(), BTreeMap::new()));
    static STAGES: RefCell<BTreeMap<&'static str, Stage>> = RefCell::default();
    static NOTIFIES: RefCell<BTreeMap<&'static str, u32>> = RefCell::default();
    /// The renders and timed stages since the main run loop last woke, by name.
    static SINCE_TICK: RefCell<BTreeMap<&'static str, (u32, Duration)>> = RefCell::default();
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
    note_since_tick(view, Duration::ZERO);
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
    if !matches!(stage, "stall" | "missed frame") {
        note_since_tick(stage, took);
    }
    STAGES.with_borrow_mut(|stages| {
        let stage = stages.entry(stage).or_default();
        stage.count += 1;
        stage.total += took;
        stage.longest = stage.longest.max(took);
    });
}

fn note_since_tick(name: &'static str, took: Duration) {
    SINCE_TICK.with_borrow_mut(|since| {
        let (count, total) = since.entry(name).or_default();
        *count += 1;
        *total += took;
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

/// Watches the main thread for stalls and prints the timed stages once a second. A stall is one
/// wake of the main run loop: from when it stops waiting to when it waits again. It is timed on
/// the wall clock, with the thread's CPU time beside it, so time spent waiting to be scheduled,
/// or blocked, shows as the difference.
pub(super) fn watch_stalls(cx: &mut gpui::App) {
    if !*ENABLED {
        return;
    }
    run_loop::observe(on_run_loop);
    eprintln!("main thread QoS: {:?}", main_thread_qos());
    cx.spawn(async move |cx| {
        let mut since = Instant::now();
        let mut cpu_since = thread_cpu();
        loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let cpu = thread_cpu();
            print_stages(since.elapsed(), cpu - cpu_since);
            (since, cpu_since) = (Instant::now(), cpu);
        }
    })
    .detach();
}

thread_local! {
    /// When the main run loop woke, by the wall clock and the thread's CPU clock.
    static WOKE: std::cell::Cell<Option<(Instant, Duration)>> = const { std::cell::Cell::new(None) };
}

fn on_run_loop(activity: run_loop::Activity) {
    match activity {
        run_loop::Activity::Woke => {
            SINCE_TICK.take();
            WOKE.set(Some((Instant::now(), thread_cpu())));
        }
        run_loop::Activity::Waits => {
            let Some((woke, cpu)) = WOKE.take() else {
                return;
            };
            let stall = woke.elapsed();
            record("stall", stall);
            if stall > FRAME {
                record("missed frame", stall);
                print_stall(stall, thread_cpu() - cpu, &SINCE_TICK.take());
            }
        }
    }
}

/// The main run loop's wakes, from a Core Foundation observer.
mod run_loop {
    use std::ffi::c_void;

    pub(super) enum Activity {
        Woke,
        Waits,
    }

    const BEFORE_WAITING: usize = 1 << 5;
    const AFTER_WAITING: usize = 1 << 6;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFRunLoopCommonModes: *const c_void;
        fn CFRunLoopGetMain() -> *mut c_void;
        fn CFRunLoopObserverCreate(
            allocator: *const c_void,
            activities: usize,
            repeats: u8,
            order: isize,
            callout: extern "C" fn(*mut c_void, usize, *mut c_void),
            context: *const c_void,
        ) -> *mut c_void;
        fn CFRunLoopAddObserver(run_loop: *mut c_void, observer: *mut c_void, mode: *const c_void);
    }

    static HANDLER: std::sync::OnceLock<fn(Activity)> = std::sync::OnceLock::new();

    extern "C" fn callout(_: *mut c_void, activity: usize, _: *mut c_void) {
        if let Some(handler) = HANDLER.get() {
            handler(match activity {
                AFTER_WAITING => Activity::Woke,
                _ => Activity::Waits,
            });
        }
    }

    /// Calls `handler` on the main thread each time its run loop wakes and waits again.
    pub(super) fn observe(handler: fn(Activity)) {
        if HANDLER.set(handler).is_err() {
            return;
        }
        // SAFETY: called on the main thread, with a callout that lives for the program; the
        // observer is kept by the run loop and never released.
        unsafe {
            let activities = BEFORE_WAITING | AFTER_WAITING;
            let observer = CFRunLoopObserverCreate(
                std::ptr::null(),
                activities,
                1,
                0,
                callout,
                std::ptr::null(),
            );
            CFRunLoopAddObserver(CFRunLoopGetMain(), observer, kCFRunLoopCommonModes);
        }
    }
}

fn main_thread_qos() -> libc::qos_class_t {
    let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
    // SAFETY: `class` is valid to write to, and a null priority is allowed.
    unsafe {
        libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, std::ptr::null_mut())
    };
    class
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

fn print_stall(stall: Duration, cpu: Duration, ran: &BTreeMap<&'static str, (u32, Duration)>) {
    let ran: Vec<String> = (ran.iter())
        .map(|(name, (count, total))| match total.is_zero() {
            true => format!("{name}×{count}"),
            false => format!("{name}×{count} {:.1}ms", total.as_secs_f64() * 1000.),
        })
        .collect();
    eprintln!(
        "stall {:.1}ms (cpu {:.1}ms) ran: {}",
        stall.as_secs_f64() * 1000.,
        cpu.as_secs_f64() * 1000.,
        ran.join(", ")
    );
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
