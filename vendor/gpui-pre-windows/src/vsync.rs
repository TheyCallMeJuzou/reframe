use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use gpui::FrameRequestSource;
use gpui_util::ResultExt;
use windows::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{DWM_TIMING_INFO, DwmFlush, DwmGetCompositionTimingInfo},
    System::Performance::QueryPerformanceFrequency,
};

static QPC_TICKS_PER_SECOND: LazyLock<u64> = LazyLock::new(|| {
    let mut frequency = 0;
    // On systems that run Windows XP or later, the function will always succeed and
    // will thus never return zero.
    unsafe { QueryPerformanceFrequency(&mut frequency).unwrap() };
    frequency as u64
});

const VSYNC_INTERVAL_THRESHOLD: Duration = Duration::from_millis(1);
const DEFAULT_VSYNC_INTERVAL: Duration = Duration::from_micros(16_666); // ~60Hz

pub(crate) struct VSyncProvider {
    interval: Duration,
    f: Box<dyn Fn() -> bool>,
}

impl VSyncProvider {
    pub(crate) fn new() -> Self {
        let interval = get_dwm_interval()
            .context("Failed to get DWM interval")
            .log_err()
            .unwrap_or(DEFAULT_VSYNC_INTERVAL);
        let f = Box::new(|| unsafe { DwmFlush().is_ok() });
        Self { interval, f }
    }

    pub(crate) fn wait_for_vsync(&self) -> FrameRequestSource {
        let vsync_start = Instant::now();
        let wait_succeeded = (self.f)();
        let elapsed = vsync_start.elapsed();
        // DwmFlush and DCompositionWaitForCompositorClock returns very early
        // instead of waiting until vblank when the monitor goes to sleep or is
        // unplugged (nothing to present due to desktop occlusion). We use 1ms as
        // a threshold for the duration of the wait functions and fallback to
        // Sleep() if it returns before that. This could happen during normal
        // operation for the first call after the vsync thread becomes non-idle,
        // but it shouldn't happen often.
        if !wait_succeeded || elapsed < VSYNC_INTERVAL_THRESHOLD {
            log::trace!("VSyncProvider::wait_for_vsync() took less time than expected");
            std::thread::sleep(self.interval);
            FrameRequestSource::LocalSchedule
        } else {
            FrameRequestSource::NativeCallback
        }
    }
}

fn get_dwm_interval() -> Result<Duration> {
    let mut timing_info = DWM_TIMING_INFO {
        cbSize: std::mem::size_of::<DWM_TIMING_INFO>() as u32,
        ..Default::default()
    };
    unsafe { DwmGetCompositionTimingInfo(HWND::default(), &mut timing_info) }?;
    let interval = retrieve_duration(timing_info.qpcRefreshPeriod, *QPC_TICKS_PER_SECOND);
    // Check for interval values that are impossibly low. A 29 microsecond
    // interval was seen (from a qpcRefreshPeriod of 60).
    if interval < VSYNC_INTERVAL_THRESHOLD {
        Ok(retrieve_duration(
            timing_info.rateRefresh.uiDenominator as u64,
            timing_info.rateRefresh.uiNumerator as u64,
        ))
    } else {
        Ok(interval)
    }
}

#[inline]
fn retrieve_duration(counts: u64, ticks_per_second: u64) -> Duration {
    let ticks_per_microsecond = ticks_per_second / 1_000_000;
    Duration::from_micros(counts / ticks_per_microsecond)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_compositor_wait_reports_native_callback() {
        let provider = VSyncProvider {
            interval: Duration::ZERO,
            f: Box::new(|| {
                std::thread::sleep(VSYNC_INTERVAL_THRESHOLD * 2);
                true
            }),
        };

        assert_eq!(
            provider.wait_for_vsync(),
            FrameRequestSource::NativeCallback
        );
    }

    #[test]
    fn failed_compositor_wait_reports_local_schedule_even_after_threshold() {
        let provider = VSyncProvider {
            interval: Duration::ZERO,
            f: Box::new(|| {
                std::thread::sleep(VSYNC_INTERVAL_THRESHOLD * 2);
                false
            }),
        };

        assert_eq!(provider.wait_for_vsync(), FrameRequestSource::LocalSchedule);
    }
}


// ---------- reframe: frames only on demand ----------

/// After this many ticks in a row that drew nothing, a window stops being ticked until GPUI
/// wakes it (its views changed, an animation runs, a frame was throttled).
const IDLE_TICKS: u32 = 3;

/// Whether a window wants frames. The vsync thread invalidates only windows that do, and sleeps
/// while none does: an idle window costs no wake-ups. (reframe's change; upstream ticks every
/// window on every vsync.)
pub(crate) struct FrameDemand {
    wanted: std::sync::atomic::AtomicBool,
    /// Woken since the last tick.
    woken: std::sync::atomic::AtomicBool,
    /// Ticks in a row that drew nothing.
    idle: std::sync::atomic::AtomicU32,
}

impl FrameDemand {
    pub(crate) fn new() -> Self {
        Self {
            wanted: std::sync::atomic::AtomicBool::new(true),
            woken: std::sync::atomic::AtomicBool::new(false),
            idle: std::sync::atomic::AtomicU32::new(0),
        }
    }

    /// The window wants frames again.
    pub(crate) fn want(&self) {
        use std::sync::atomic::Ordering;
        self.woken.store(true, Ordering::Release);
        self.idle.store(0, Ordering::Release);
        self.wanted.store(true, Ordering::Release);
        FRAME_CLOCK.wake();
    }

    pub(crate) fn wanted(&self) -> bool {
        self.wanted.load(std::sync::atomic::Ordering::Acquire)
    }

    /// After a tick's frame request: whether it drew decides whether the window goes on being
    /// ticked.
    pub(crate) fn ticked(&self, drew: bool) {
        use std::sync::atomic::Ordering;
        if drew || self.woken.swap(false, Ordering::AcqRel) {
            self.idle.store(0, Ordering::Release);
        } else if self.idle.fetch_add(1, Ordering::AcqRel) + 1 >= IDLE_TICKS {
            self.wanted.store(false, Ordering::Release);
        }
    }
}

/// What the vsync thread sleeps on while no window wants frames.
pub(crate) struct FrameClock {
    woken: std::sync::Mutex<bool>,
    wake: std::sync::Condvar,
}

pub(crate) static FRAME_CLOCK: LazyLock<FrameClock> = LazyLock::new(|| FrameClock {
    woken: std::sync::Mutex::new(false),
    wake: std::sync::Condvar::new(),
});

impl FrameClock {
    pub(crate) fn wake(&self) {
        let mut woken = self.woken.lock().unwrap_or_else(|error| error.into_inner());
        *woken = true;
        self.wake.notify_all();
    }

    /// Sleeps until something wants frames (or the thread is to stop: that wakes it too).
    pub(crate) fn sleep(&self) {
        let mut woken = self.woken.lock().unwrap_or_else(|error| error.into_inner());
        while !*woken {
            woken = self.wake.wait(woken).unwrap_or_else(|error| error.into_inner());
        }
        *woken = false;
    }
}
