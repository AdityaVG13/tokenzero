//! Seeded deterministic schedule replay and small-history linearizability checking.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeterministicSchedule {
    pub seed: u64,
    pub order: Vec<usize>,
}
impl DeterministicSchedule {
    pub fn generate(seed: u64, width: usize) -> Self {
        let mut order: (Vec<usize>) = (0..width).collect();
        let mut x = seed;
        for i in (1..width).rev() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            order.swap(i, (x as usize) % (i + 1));
        }
        Self { seed, order }
    }
    pub fn replay<T>(&self, mut step: impl FnMut(usize) -> T) -> Result<Vec<T>, String> {
        let expected: BTreeSet<_> = (0..self.order.len()).collect();
        if self.order.iter().copied().collect::<BTreeSet<_>>() != expected {
            return Err("schedule is not a permutation".into());
        }
        Ok(self.order.iter().copied().map(&mut step).collect())
    }
    pub fn replay_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("schedule serialization")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum HistoryOp {
    Mint {
        ref_id: String,
    },
    AliasCas {
        alias: String,
        expected: Option<String>,
        new: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub process: u32,
    pub invoke: u64,
    pub complete: u64,
    pub op: HistoryOp,
    pub success: bool,
}

pub fn check_linearizable(history: &[HistoryEvent]) -> Result<(), String> {
    if history.iter().any(|e| e.invoke >= e.complete) {
        return Err("invocation must precede completion".into());
    }
    let mut before = vec![Vec::new(); history.len()];
    for (i, a) in history.iter().enumerate() {
        for (j, b) in history.iter().enumerate() {
            if i != j && a.complete < b.invoke {
                before[j].push(i);
            }
        }
    }
    fn search(
        history: &[HistoryEvent],
        before: &[Vec<usize>],
        done: &mut BTreeSet<usize>,
        minted: &mut BTreeSet<String>,
        aliases: &mut BTreeMap<String, String>,
    ) -> bool {
        if done.len() == history.len() {
            return true;
        }
        for i in 0..history.len() {
            if done.contains(&i) || before[i].iter().any(|p| !done.contains(p)) {
                continue;
            }
            let mut next_m = minted.clone();
            let mut next_a = aliases.clone();
            let valid = match &history[i].op {
                HistoryOp::Mint { ref_id } => {
                    let inserted = next_m.insert(ref_id.clone());
                    history[i].success == inserted || (history[i].success && !inserted)
                }
                HistoryOp::AliasCas {
                    alias,
                    expected,
                    new,
                } => {
                    let matches = next_a.get(alias) == expected.as_ref();
                    if matches {
                        next_a.insert(alias.clone(), new.clone());
                    }
                    history[i].success == matches
                }
            };
            if valid {
                done.insert(i);
                if search(history, before, done, &mut next_m, &mut next_a) {
                    return true;
                }
                done.remove(&i);
            }
        }
        false
    }
    if search(
        history,
        &before,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
        &mut BTreeMap::new(),
    ) {
        Ok(())
    } else {
        Err("history is not linearizable".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seeded_schedules_are_bitwise_replayable() {
        let a = DeterministicSchedule::generate(0x5eed, 32);
        let bytes = a.replay_bytes();
        let b: DeterministicSchedule = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(bytes, b.replay_bytes());
        assert_eq!(a.replay(|i| i * i).unwrap(), b.replay(|i| i * i).unwrap());
    }
    #[test]
    fn elle_style_mint_and_alias_cas_histories() {
        let h = vec![
            HistoryEvent {
                process: 1,
                invoke: 1,
                complete: 4,
                op: HistoryOp::Mint { ref_id: "r".into() },
                success: true,
            },
            HistoryEvent {
                process: 2,
                invoke: 2,
                complete: 5,
                op: HistoryOp::AliasCas {
                    alias: "a".into(),
                    expected: None,
                    new: "r".into(),
                },
                success: true,
            },
            HistoryEvent {
                process: 3,
                invoke: 6,
                complete: 7,
                op: HistoryOp::AliasCas {
                    alias: "a".into(),
                    expected: None,
                    new: "x".into(),
                },
                success: false,
            },
        ];
        assert!(check_linearizable(&h).is_ok());
        let mut bad = h;
        bad[2].success = true;
        assert!(check_linearizable(&bad).is_err());
    }
}
