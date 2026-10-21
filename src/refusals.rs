//! A run's ledger of refused calls (Glut robust, #112, `docs/plans/glut-robust.md` R1–R5): a model that is refused
//! keeps asking for the same call, or goes around it, until the step limit — measured 30 identical fetches and
//! `curl`, `python` and a subagent after them. The ledger warns the model once and then ends the run.
//!
//! The same refusal is the same tool and target as the gate judged them (`bash` and its command, `web_fetch` and its
//! URL, `Network` and the host) or a hook's block of the same tool and arguments. Ways around a refusal cannot be
//! told apart safely, so beside the identical ones every refusal of the run is counted.

use std::collections::HashMap;

use crate::config::RefusalLimits;

/// How much of a target an ending message names: a whole command line can be long.
const MAX_TARGET_CHARS: usize = 120;

/// One refusal as the run saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub tool: String,
    pub target: String,
    pub reason: String,
}

/// The run's refusals so far.
#[derive(Debug)]
pub(crate) struct Ledger {
    limits: RefusalLimits,
    /// Per tool and target, how often it was refused.
    seen: HashMap<(String, String), u32>,
    /// Every refusal of the run, a subagent's included.
    total: u32,
    /// Why the run must end, once a limit is reached.
    ended: Option<String>,
}

impl Ledger {
    pub fn new(limits: RefusalLimits) -> Self {
        Ledger {
            limits,
            seen: HashMap::new(),
            total: 0,
            ended: None,
        }
    }

    /// How many refusals the run had, for the parent of a subagent (R5).
    pub fn total(&self) -> u32 {
        self.total
    }

    /// The message the run ends with, once a limit was reached.
    pub fn ended(&self) -> Option<&str> {
        self.ended.as_deref()
    }

    /// Records the refusals of one call — and those a subagent it started had (`from_subagent`) — and returns the
    /// note its answer gets: none while nothing repeats and the run's count is low.
    pub fn record(&mut self, refused: &[Refusal], from_subagent: u32) -> Option<String> {
        if refused.is_empty() && from_subagent == 0 {
            return None;
        }
        let mut notes = Vec::new();
        // One refusal per tool and target and call: the proxy judges every connection, and a single `npm install`
        // retrying a denied host is one attempt of the model's, not three.
        let mut once = std::collections::HashSet::new();
        for refusal in refused
            .iter()
            .filter(|refusal| once.insert((refusal.tool.as_str(), refusal.target.as_str())))
        {
            self.total += 1;
            let key = (refusal.tool.clone(), refusal.target.clone());
            let count = self.seen.entry(key).or_insert(0);
            *count += 1;
            let count = *count;
            if count >= self.limits.repeat {
                self.end_with(format!(
                    "stopped: {} was refused {count} times ({})",
                    named(refusal),
                    refusal.reason
                ));
            } else if count + 1 == self.limits.repeat {
                notes.push(format!(
                    "[Glut: this call was refused {count} times now ({}). Do not try it again, nor around it — curl, \
                     python, a script or a subagent are refused too. Go on without it, or finish and say what is \
                     missing.]",
                    refusal.reason
                ));
            }
        }
        self.total += from_subagent;
        if self.total >= self.limits.total {
            let last = refused
                .last()
                .map(|refusal| format!(" (the last: {}, {})", named(refusal), refusal.reason))
                .unwrap_or_default();
            self.end_with(format!(
                "stopped: {} calls were refused in this run{last}",
                self.total
            ));
        } else if self.total >= self.limits.total_note_from() {
            notes.push(format!(
                "[Glut: {} calls were refused in this run; at {} it ends. Finish with what you have, or say what is \
                 missing.]",
                self.total, self.limits.total
            ));
        }
        (!notes.is_empty()).then(|| notes.join("\n"))
    }

    /// Keeps the first reason: the call that reached a limit is the one to name.
    fn end_with(&mut self, message: String) {
        self.ended.get_or_insert(message);
    }
}

/// `bash` `git push` — the tool and the start of what it was asked for.
fn named(refusal: &Refusal) -> String {
    let target: String = refusal.target.chars().take(MAX_TARGET_CHARS).collect();
    let cut = if target.len() < refusal.target.len() {
        "…"
    } else {
        ""
    };
    if target.is_empty() {
        format!("`{}`", refusal.tool)
    } else {
        format!("`{}` {target}{cut}", refusal.tool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(tool: &str, target: &str) -> Refusal {
        Refusal {
            tool: tool.into(),
            target: target.into(),
            reason: "no one to ask".into(),
        }
    }

    #[test]
    fn the_second_identical_refusal_warns_and_the_third_ends_the_run() {
        let mut ledger = Ledger::new(RefusalLimits::DEFAULT);
        let fetch = refusal("web_fetch", "https://example.com/a");
        assert_eq!(ledger.record(std::slice::from_ref(&fetch), 0), None);
        let note = ledger.record(std::slice::from_ref(&fetch), 0).unwrap();
        assert!(
            note.contains("refused 2 times now (no one to ask)"),
            "{note}"
        );
        assert!(note.contains("nor around it"), "{note}");
        assert_eq!(ledger.ended(), None);
        ledger.record(std::slice::from_ref(&fetch), 0);
        assert_eq!(
            ledger.ended(),
            Some("stopped: `web_fetch` https://example.com/a was refused 3 times (no one to ask)")
        );
    }

    #[test]
    fn different_refusals_only_count_toward_the_total_which_warns_then_ends() {
        let mut ledger = Ledger::new(RefusalLimits {
            repeat: 3,
            total: 6,
        });
        assert_eq!(ledger.record(&[refusal("bash", "curl a")], 0), None);
        let note = ledger.record(&[refusal("bash", "python3 b")], 0).unwrap();
        assert!(
            note.contains("2 calls were refused in this run; at 6 it ends"),
            "{note}"
        );
        for n in 3..6 {
            ledger.record(&[refusal("bash", &format!("try {n}"))], 0);
            assert_eq!(ledger.ended(), None);
        }
        ledger.record(&[refusal("bash", "try 6")], 0);
        assert_eq!(
            ledger.ended(),
            Some(
                "stopped: 6 calls were refused in this run (the last: `bash` try 6, no one to ask)"
            )
        );
    }

    #[test]
    fn a_subagents_refusals_count_toward_the_parents_total() {
        let mut ledger = Ledger::new(RefusalLimits {
            repeat: 3,
            total: 6,
        });
        ledger.record(&[refusal("task", "")], 5);
        assert_eq!(ledger.total(), 6);
        assert_eq!(
            ledger.ended(),
            Some("stopped: 6 calls were refused in this run (the last: `task`, no one to ask)")
        );
    }

    #[test]
    fn the_first_limit_reached_names_the_ending_and_a_long_target_is_cut() {
        let mut ledger = Ledger::new(RefusalLimits {
            repeat: 2,
            total: 2,
        });
        let long = "x".repeat(MAX_TARGET_CHARS + 10);
        ledger.record(&[refusal("bash", &long)], 0);
        ledger.record(&[refusal("bash", &long)], 0);
        let ended = ledger.ended().unwrap();
        assert!(ended.starts_with("stopped: `bash` xxx"), "{ended}");
        assert!(
            ended.contains("…") && ended.contains("refused 2 times"),
            "{ended}"
        );
    }

    #[test]
    fn the_same_target_refused_twice_within_one_call_counts_once() {
        let mut ledger = Ledger::new(RefusalLimits::DEFAULT);
        let host = refusal("Network", "registry.npmjs.org");
        ledger.record(&[host.clone(), host.clone(), host.clone()], 0);
        assert_eq!(ledger.total(), 1);
        assert_eq!(ledger.ended(), None);
    }

    #[test]
    fn nothing_refused_says_nothing() {
        let mut ledger = Ledger::new(RefusalLimits::DEFAULT);
        assert_eq!(ledger.record(&[], 0), None);
        assert_eq!(ledger.total(), 0);
    }
}
