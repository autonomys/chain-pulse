//! slot monitor

use crate::cli::SlotsConfig;
use crate::error::Error;
use crate::p2p_network::{PoTInfo, PoTStream};
use crate::slack::{Alert, AlertSink};
use humantime::format_duration;
use log::{debug, error, info};
use shared::subspace::Slot;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::time;

const SLOT_WINDOW_SIZE: usize = 600;

#[derive(Debug, Clone, Copy)]
struct SlotDetails {
    slot: Slot,
    duration: Duration,
}

#[derive(Debug)]
struct SlotWindow {
    slots: VecDeque<SlotDetails>,
    total: Duration,
}

impl SlotWindow {
    fn new() -> Self {
        Self {
            slots: VecDeque::with_capacity(SLOT_WINDOW_SIZE),
            total: Duration::ZERO,
        }
    }

    fn push(&mut self, details: SlotDetails) {
        if self.is_full() {
            let evicted = self
                .slots
                .pop_front()
                .map_or(Duration::ZERO, |oldest| oldest.duration);
            self.total -= evicted;
        }
        self.total += details.duration;
        self.slots.push_back(details);
    }

    fn last(&self) -> Option<&SlotDetails> {
        self.slots.back()
    }

    fn len(&self) -> usize {
        self.slots.len()
    }

    fn is_full(&self) -> bool {
        self.slots.len() >= SLOT_WINDOW_SIZE
    }

    fn average(&self) -> Duration {
        self.total
            .checked_div(self.slots.len() as u32)
            .unwrap_or_default()
    }
}

#[derive(Debug)]
pub(crate) struct TimekeeperStall {
    pub(crate) last_slot: Slot,
    pub(crate) duration: Duration,
}

#[derive(Debug)]
pub(crate) struct TimekeeperRecovery {
    pub(crate) slot: Slot,
    pub(crate) duration: Duration,
}

#[derive(Debug)]
pub(crate) struct SlowSlot {
    pub(crate) slot: Slot,
    pub(crate) previous_slot: Slot,
    pub(crate) slot_time: Duration,
    pub(crate) threshold: Duration,
}

#[derive(Debug)]
pub(crate) struct AvgSlowSlot {
    pub(crate) slot: Slot,
    pub(crate) slot_count: usize,
    pub(crate) avg_slot_time: Duration,
    pub(crate) threshold: Duration,
}

pub(crate) async fn monitor_slots(
    mut slot_stream: PoTStream,
    config: SlotsConfig,
    alert_sink: AlertSink,
) -> Result<(), Error> {
    info!("🚀 Starting slot monitor with config {config:?} ...");
    let slot_timeout = Duration::from_secs(30);
    let mut timeout_fired = None;
    let mut last_instant = Instant::now();
    let mut slots = SlotWindow::new();
    loop {
        match time::timeout(slot_timeout, slot_stream.recv()).await {
            Ok(slot) => {
                let PoTInfo { at: now, proof } = slot?;
                debug!("Received slot {}", proof.slot);
                let duration = now.duration_since(last_instant);
                let Some(last_slot) = slots.last() else {
                    slots.push(SlotDetails {
                        slot: proof.slot,
                        duration,
                    });
                    last_instant = now;
                    continue;
                };

                if proof.slot != last_slot.slot + 1 {
                    continue;
                }

                info!(
                    "✅ Timekeeper chain extended from[{}] to [{}] in {}",
                    last_slot.slot,
                    proof.slot,
                    format_duration(duration)
                );

                let last_timeout = timeout_fired.take();
                if let Some(timeout) = last_timeout {
                    info!(
                        "✅ Timekeeper recovered: slot[{}] after: {} ⏱️",
                        proof.slot,
                        format_duration(timeout)
                    );

                    let alert = Alert::TimekeeperRecovery(TimekeeperRecovery {
                        slot: proof.slot,
                        duration: timeout,
                    });

                    if let Err(err) = alert_sink.send(alert) {
                        error!("⛔️ failed to send Timekeeper recovery alert: {err}");
                    }
                }

                if duration.gt(config.per_slot_threshold.as_ref()) {
                    info!(
                        "⛔️ Per slot threshold breached: Last Slot[{}] New Slot[{}] Time[{}]",
                        last_slot.slot,
                        proof.slot,
                        format_duration(duration)
                    );

                    let alert = Alert::SlowSlot(SlowSlot {
                        slot: proof.slot,
                        previous_slot: last_slot.slot,
                        slot_time: duration,
                        threshold: config.per_slot_threshold.into(),
                    });

                    if let Err(err) = alert_sink.send(alert) {
                        error!("⛔️ failed to send Slow slot alert: {err}");
                    }
                }

                // extension
                let slot_details = SlotDetails {
                    slot: proof.slot,
                    duration,
                };
                last_instant = now;

                slots.push(slot_details);
                let length = slots.len();
                let avg_slot_time = slots.average();
                if slots.is_full() && avg_slot_time.gt(config.avg_slot_threshold.as_ref()) {
                    info!(
                        "⛔️ Average slot threshold breached: Slots: {}: Avg Time: {}: Threshold: {}",
                        length,
                        format_duration(avg_slot_time),
                        config.avg_slot_threshold
                    );

                    let alert = Alert::AvgSlowSlots(AvgSlowSlot {
                        slot: proof.slot,
                        slot_count: length,
                        threshold: config.avg_slot_threshold.into(),
                        avg_slot_time,
                    });

                    if let Err(err) = alert_sink.send(alert) {
                        error!("⛔️ failed to send Avg Slow slot alert: {err}");
                    }
                }
            }
            Err(_) => {
                let non_import_duration = timeout_fired
                    .map(|timeout| timeout.saturating_add(slot_timeout))
                    .unwrap_or(slot_timeout);
                error!(
                    "⛔️ Timekeeper stalled! No slots in last {} 🕒",
                    format_duration(non_import_duration)
                );
                if let Some(last_slot) = slots.last() {
                    let alert = Alert::TimekeeperStall(TimekeeperStall {
                        last_slot: last_slot.slot,
                        duration: non_import_duration,
                    });
                    if let Err(err) = alert_sink.send(alert) {
                        error!("⛔️ failed to send timekeeper stall alert: {err}");
                    }
                }

                timeout_fired = Some(non_import_duration);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_slots(window: &mut SlotWindow, next_slot: &mut Slot, count: usize, duration: Duration) {
        for _ in 0..count {
            window.push(SlotDetails {
                slot: *next_slot,
                duration,
            });
            *next_slot += 1;
        }
    }

    #[test]
    fn test_slot_window_average_covers_only_latest_slots() {
        let mut window = SlotWindow::new();
        let mut next_slot = 0;

        push_slots(
            &mut window,
            &mut next_slot,
            SLOT_WINDOW_SIZE - 1,
            Duration::from_secs(2),
        );
        assert!(!window.is_full());

        push_slots(&mut window, &mut next_slot, 1, Duration::from_secs(2));
        assert!(window.is_full());
        assert_eq!(window.average(), Duration::from_secs(2));

        push_slots(
            &mut window,
            &mut next_slot,
            SLOT_WINDOW_SIZE / 2,
            Duration::from_secs(1),
        );
        assert!(window.is_full());
        assert_eq!(window.average(), Duration::from_millis(1500));

        push_slots(
            &mut window,
            &mut next_slot,
            SLOT_WINDOW_SIZE / 2,
            Duration::from_secs(1),
        );
        assert!(window.is_full());
        assert_eq!(window.len(), SLOT_WINDOW_SIZE);
        assert_eq!(window.average(), Duration::from_secs(1));
        assert_eq!(window.last().map(|last| last.slot), Some(next_slot - 1));
    }
}
