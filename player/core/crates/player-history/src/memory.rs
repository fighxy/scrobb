//! История в памяти: для тестов и как образец поведения для настоящих хранилищ.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use crate::{
    DeliveryState, HistoryError, HistoryStore, NewPlay, ORIGIN_PLAYER, Outcome, PlayId, PlayRecord,
    QUEUE_MAX_AGE_MS, QUEUE_MAX_ITEMS, Settlement,
};

/// История в памяти.
#[derive(Debug)]
pub struct MemoryHistory {
    device_id: String,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    plays: HashMap<PlayId, PlayRecord>,
    deliveries: HashMap<(PlayId, String), Delivery>,
}

#[derive(Debug, Clone)]
struct Delivery {
    state: DeliveryState,
    reason: Option<String>,
}

impl Default for MemoryHistory {
    fn default() -> Self {
        Self::new("memory")
    }
}

impl MemoryHistory {
    /// Пустая история устройства `device_id`.
    pub fn new(device_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            state: Mutex::default(),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl State {
    fn play(&mut self, id: &PlayId) -> Result<&mut PlayRecord, HistoryError> {
        self.plays
            .get_mut(id)
            .ok_or_else(|| HistoryError(format!("нет прослушивания {}", id.0)))
    }

    /// Ждущие отправки в `target`, самые старые первыми.
    fn pending(&self, target: &str) -> Vec<&PlayRecord> {
        let mut plays: Vec<_> = self
            .deliveries
            .iter()
            .filter(|((_, t), d)| t == target && d.state == DeliveryState::Pending)
            .filter_map(|((id, _), _)| self.plays.get(id))
            .collect();
        plays.sort_by(|a, b| (a.started_at_ms, &a.id).cmp(&(b.started_at_ms, &b.id)));
        plays
    }
}

impl HistoryStore for MemoryHistory {
    fn begin(&self, play: &NewPlay) -> Result<(), HistoryError> {
        let mut s = self.state();
        if s.plays.contains_key(&play.id) {
            return Err(HistoryError(format!(
                "прослушивание {} уже есть",
                play.id.0
            )));
        }
        s.plays.insert(
            play.id.clone(),
            PlayRecord {
                id: play.id.clone(),
                started_at_ms: play.started_at_ms,
                ended_at_ms: None,
                played_ms: 0,
                outcome: Outcome::Playing,
                scrobbled: false,
                track: play.track.clone(),
                track_id: play.track_id,
                device_id: self.device_id.clone(),
                origin: ORIGIN_PLAYER.into(),
            },
        );
        Ok(())
    }

    fn checkpoint(&self, id: &PlayId, played_ms: u64) -> Result<(), HistoryError> {
        self.state().play(id)?.played_ms = played_ms;
        Ok(())
    }

    fn mark_scrobbled(
        &self,
        id: &PlayId,
        played_ms: u64,
        targets: &[&str],
        _now_ms: i64,
    ) -> Result<(), HistoryError> {
        let mut s = self.state();
        let play = s.play(id)?;
        play.scrobbled = true;
        play.played_ms = played_ms;
        for target in targets {
            s.deliveries
                .entry((id.clone(), (*target).to_owned()))
                .or_insert(Delivery {
                    state: DeliveryState::Pending,
                    reason: None,
                });
        }
        Ok(())
    }

    fn finish(
        &self,
        id: &PlayId,
        ended_at_ms: i64,
        played_ms: u64,
        outcome: Outcome,
    ) -> Result<(), HistoryError> {
        let mut s = self.state();
        let play = s.play(id)?;
        play.ended_at_ms = Some(ended_at_ms);
        play.played_ms = played_ms;
        play.outcome = outcome;
        Ok(())
    }

    fn recover_interrupted(&self) -> Result<usize, HistoryError> {
        let mut s = self.state();
        let mut closed = 0;
        for play in s.plays.values_mut() {
            if play.outcome == Outcome::Playing {
                play.outcome = Outcome::Interrupted;
                play.ended_at_ms = Some(play.started_at_ms + play.played_ms as i64);
                closed += 1;
            }
        }
        Ok(closed)
    }

    fn pending(&self, target: &str, limit: usize) -> Result<Vec<PlayRecord>, HistoryError> {
        let s = self.state();
        Ok(s.pending(target).into_iter().take(limit).cloned().collect())
    }

    fn pending_count(&self, target: &str) -> Result<usize, HistoryError> {
        Ok(self.state().pending(target).len())
    }

    fn settle(
        &self,
        target: &str,
        results: &[Settlement],
        _now_ms: i64,
    ) -> Result<(), HistoryError> {
        let mut s = self.state();
        for r in results {
            if let Some(d) = s.deliveries.get_mut(&(r.id.clone(), target.to_owned())) {
                d.state = r.state;
                d.reason = r.reason.clone();
            }
        }
        Ok(())
    }

    fn expire(&self, target: &str, now_ms: i64) -> Result<usize, HistoryError> {
        let mut s = self.state();
        let pending: Vec<_> = s
            .pending(target)
            .into_iter()
            .map(|p| (p.id.clone(), p.started_at_ms))
            .collect();
        let too_old = pending
            .iter()
            .filter(|(_, at)| *at < now_ms - QUEUE_MAX_AGE_MS)
            .count();
        // Старые и так идут первыми: истекают они и самые старые сверх лимита.
        let expired = too_old.max(pending.len().saturating_sub(QUEUE_MAX_ITEMS));
        for (id, _) in &pending[..expired] {
            if let Some(d) = s.deliveries.get_mut(&(id.clone(), target.to_owned())) {
                d.state = DeliveryState::Expired;
            }
        }
        Ok(expired)
    }

    fn recent(
        &self,
        before_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<PlayRecord>, HistoryError> {
        let s = self.state();
        let mut plays: Vec<_> = s
            .plays
            .values()
            .filter(|p| before_ms.is_none_or(|before| p.started_at_ms < before))
            .collect();
        plays.sort_by(|a, b| (b.started_at_ms, &b.id).cmp(&(a.started_at_ms, &a.id)));
        Ok(plays.into_iter().take(limit).cloned().collect())
    }
}
