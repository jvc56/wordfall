//! An in-memory cascade that applies operations through the rules, in the
//! order the server checks them. The shared vectors and the property test
//! run against it; the sync endpoint applies the same checks and rules to
//! database rows.

use std::collections::{BTreeMap, HashMap, HashSet};

use uuid::Uuid;

use super::order::{questions_hash, shuffle};
use super::rows::{Origin, Progression};
use super::rules::{self, CascadeState, FinishResult, Grade, Opts, QuizState, Reason, SegmentResult};

#[derive(Debug, Clone)]
pub struct SimQuiz {
    pub id: Uuid,
    pub state: QuizState,
    pub origin_quiz_id: Option<Uuid>,
    pub origin_attempt: Option<u32>,
    pub origin_segment_end: Option<u32>,
    /// Ascending.
    pub questions: Vec<u32>,
    pub grades: HashMap<u32, Grade>,
}

impl SimQuiz {
    pub fn positions(&self) -> Vec<u32> {
        shuffle(&self.questions, self.state.seed)
    }

    pub fn correct(&self) -> u32 {
        self.grades.values().filter(|g| **g == Grade::Correct).count() as u32
    }

    pub fn missed(&self) -> u32 {
        self.grades.values().filter(|g| **g == Grade::Missed).count() as u32
    }

    fn reset(&mut self, seed: u64) {
        self.state.attempt += 1;
        self.state.seed = seed;
        self.state.cursor = 0;
        self.state.run_start = 0;
        self.grades.clear();
    }
}

#[derive(Debug, Clone)]
pub enum Op {
    Grade { quiz: Uuid, attempt: u32, attempt_seed: u64, question_idx: u32, grade: Grade },
    MoveCursor { quiz: Uuid, attempt: u32, attempt_seed: u64, position: u32 },
    Finish { quiz: Uuid, attempt: u32, attempt_seed: u64, shuffle_seed: u64, new_quiz_id: Uuid },
    FinishSegment { quiz: Uuid, attempt: u32, attempt_seed: u64, segment_end: u32, shuffle_seed: u64, new_quiz_id: Uuid },
    RestoreQuiz { quiz: Uuid, shuffle_seed: u64 },
    TrashCascade,
    RestoreCascade,
    PurgeQuiz { quiz: Uuid },
    PurgeCascade,
    SetCascadeOptions { segment_size: Option<i64>, progression: Option<Progression>, require_alphabetical: Option<bool> },
    SetQuizOptions { quiz: Uuid, segment_size: Option<i64>, progression: Option<Progression>, require_alphabetical: Option<bool> },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub outcome: Option<String>,
    pub new_quiz_question_count: Option<u32>,
    pub new_quiz_questions_hash: Option<u64>,
    /// Levels and attempts, for a completion this model computed.
    pub completion: Option<(u32, u32)>,
}

#[derive(Debug, Clone)]
pub struct SimCascade {
    pub state: CascadeState,
    pub completions: u32,
    pub purged: bool,
    pub quizzes: BTreeMap<Uuid, SimQuiz>,
    pub purged_quizzes: HashSet<Uuid>,
    pub max_quiz_questions: u32,
}

impl SimCascade {
    pub fn new(threshold: u32, opts: Opts, count: u32, source_id: Uuid, source_seed: u64) -> Self {
        let mut quizzes = BTreeMap::new();
        quizzes.insert(
            source_id,
            SimQuiz {
                id: source_id,
                state: QuizState {
                    level: 1,
                    origin: Origin::Source,
                    segment_chain: false,
                    opts,
                    attempt: 1,
                    seed: source_seed,
                    question_count: count,
                    cursor: 0,
                    run_start: 0,
                    active: true,
                },
                origin_quiz_id: None,
                origin_attempt: None,
                origin_segment_end: None,
                questions: (0..count).collect(),
                grades: HashMap::new(),
            },
        );
        SimCascade {
            state: CascadeState {
                clear_threshold: threshold,
                opts,
                depth: 1,
                peak_depth: 1,
                attempts_since_completion: 0,
                trashed: false,
            },
            completions: 0,
            purged: false,
            quizzes,
            purged_quizzes: HashSet::new(),
            max_quiz_questions: 300_000,
        }
    }

    pub fn deepest(&self) -> Option<&SimQuiz> {
        self.quizzes.values().find(|q| q.state.active && q.state.level == self.state.depth)
    }

    fn quiz(&self, id: Uuid) -> Result<&SimQuiz, Reason> {
        if self.purged {
            return Err(Reason::NotFound);
        }
        self.quizzes.get(&id).ok_or(Reason::NotFound)
    }

    fn live(&self, id: Uuid) -> Result<&SimQuiz, Reason> {
        let q = self.quiz(id)?;
        rules::check_live(&self.state, &q.state)?;
        Ok(q)
    }

    fn id_free(&self, id: Uuid) -> Result<(), Reason> {
        if self.quizzes.contains_key(&id) || self.purged_quizzes.contains(&id) {
            return Err(Reason::Invalid);
        }
        Ok(())
    }

    fn insert(&mut self, id: Uuid, parent: Uuid, n: rules::NewQuiz) -> (u32, u64) {
        let count = n.questions.len() as u32;
        let hash = questions_hash(&n.questions);
        self.quizzes.insert(
            id,
            SimQuiz {
                id,
                state: QuizState {
                    level: n.level,
                    origin: n.origin,
                    segment_chain: n.segment_chain,
                    opts: n.opts,
                    attempt: 1,
                    seed: n.seed,
                    question_count: count,
                    cursor: 0,
                    run_start: 0,
                    active: true,
                },
                origin_quiz_id: Some(parent),
                origin_attempt: Some(n.origin_attempt),
                origin_segment_end: n.origin_segment_end,
                questions: n.questions,
                grades: HashMap::new(),
            },
        );
        (count, hash)
    }

    pub fn apply(&mut self, op: &Op) -> Result<Applied, Reason> {
        match op {
            Op::Grade { quiz, attempt, attempt_seed, question_idx, grade } => {
                let q = self.live(*quiz)?;
                rules::check_attempt(&q.state, *attempt, *attempt_seed)?;
                if q.questions.binary_search(question_idx).is_err() {
                    return Err(Reason::NotFound);
                }
                self.quizzes.get_mut(quiz).unwrap().grades.insert(*question_idx, *grade);
                Ok(Applied::default())
            }
            Op::MoveCursor { quiz, attempt, attempt_seed, position } => {
                let q = self.live(*quiz)?;
                rules::check_attempt(&q.state, *attempt, *attempt_seed)?;
                rules::check_move_cursor(&q.state, *position)?;
                self.quizzes.get_mut(quiz).unwrap().state.cursor = *position;
                Ok(Applied::default())
            }
            Op::Finish { quiz, attempt, attempt_seed, shuffle_seed, new_quiz_id } => {
                let q = self.live(*quiz)?;
                // PQ-013: the attempt before the depth.
                rules::check_attempt(&q.state, *attempt, *attempt_seed)?;
                rules::check_deepest(&self.state, &q.state)?;
                if q.grades.len() as u32 != q.state.question_count {
                    return Err(Reason::Ungraded);
                }
                self.id_free(*new_quiz_id)?;
                let q = q.clone();
                let misses: Vec<u32> =
                    q.questions.iter().copied().filter(|i| q.grades.get(i) == Some(&Grade::Missed)).collect();
                let r = rules::finish(&mut self.state, &q.state, q.correct(), misses, *shuffle_seed);
                let mut out = Applied { outcome: Some(r.outcome().as_str().to_owned()), ..Default::default() };
                match &r {
                    FinishResult::Finished { replacement, .. } => {
                        let fq = self.quizzes.get_mut(quiz).unwrap();
                        fq.state.active = false;
                        if let Some(n) = replacement {
                            let (c, h) = self.insert(*new_quiz_id, *quiz, n.clone());
                            out.new_quiz_question_count = Some(c);
                            out.new_quiz_questions_hash = Some(h);
                        }
                    }
                    FinishResult::Descended { reset_seed, new_level } => {
                        self.quizzes.get_mut(quiz).unwrap().reset(*reset_seed);
                        let (c, h) = self.insert(*new_quiz_id, *quiz, new_level.clone());
                        out.new_quiz_question_count = Some(c);
                        out.new_quiz_questions_hash = Some(h);
                    }
                    FinishResult::Completed { reset_seed, levels, attempts } => {
                        self.completions += 1;
                        out.completion = Some((*levels, *attempts));
                        self.reset_in_place(*quiz, *reset_seed, &mut out);
                    }
                    FinishResult::Reshuffled { reset_seed } => self.reset_in_place(*quiz, *reset_seed, &mut out),
                }
                Ok(out)
            }
            Op::FinishSegment { quiz, attempt, attempt_seed, segment_end, shuffle_seed, new_quiz_id } => {
                let q = self.live(*quiz)?;
                // PQ-013: the attempt and the duplicate before the depth.
                rules::check_attempt(&q.state, *attempt, *attempt_seed)?;
                rules::check_segment_end(&q.state, *segment_end)?;
                let duplicate = self.quizzes.values().any(|o| {
                    o.state.origin == Origin::Segment
                        && o.origin_quiz_id == Some(*quiz)
                        && o.origin_attempt == Some(q.state.attempt)
                        && o.origin_segment_end == Some(*segment_end)
                });
                if duplicate {
                    return Err(Reason::DuplicateSegment);
                }
                rules::check_deepest(&self.state, &q.state)?;
                rules::check_segment_past_cursor(&q.state, *segment_end)?;
                let pos = q.positions();
                if pos[..*segment_end as usize].iter().any(|i| !q.grades.contains_key(i)) {
                    return Err(Reason::Ungraded);
                }
                self.id_free(*new_quiz_id)?;
                let q = q.clone();
                let mut run_misses: Vec<u32> = pos[q.state.run_start as usize..*segment_end as usize]
                    .iter()
                    .copied()
                    .filter(|i| q.grades.get(i) == Some(&Grade::Missed))
                    .collect();
                run_misses.sort_unstable();
                let r = rules::finish_segment(&mut self.state, &q.state, run_misses, *segment_end, *shuffle_seed);
                let mut out = Applied::default();
                match r {
                    SegmentResult::Drilled(n) => {
                        let (c, h) = self.insert(*new_quiz_id, *quiz, n);
                        out.outcome = Some("drilled".into());
                        out.new_quiz_question_count = Some(c);
                        out.new_quiz_questions_hash = Some(h);
                    }
                    SegmentResult::Continued => out.outcome = Some("continued".into()),
                }
                let pq = self.quizzes.get_mut(quiz).unwrap();
                pq.state.cursor = *segment_end;
                pq.state.run_start = *segment_end;
                Ok(out)
            }
            Op::RestoreQuiz { quiz, shuffle_seed } => {
                let q = self.quiz(*quiz)?;
                if q.state.active {
                    return Err(Reason::NotCleared);
                }
                let qs = q.state.clone();
                let r = rules::restore_quiz(&mut self.state, &qs, *shuffle_seed);
                let rq = self.quizzes.get_mut(quiz).unwrap();
                rq.reset(r.seed);
                rq.state.attempt = r.attempt;
                rq.state.level = r.level;
                rq.state.opts = r.opts;
                rq.state.active = true;
                Ok(Applied {
                    new_quiz_question_count: Some(rq.state.question_count),
                    new_quiz_questions_hash: Some(questions_hash(&rq.questions)),
                    ..Default::default()
                })
            }
            Op::TrashCascade => {
                if self.purged {
                    return Err(Reason::NotFound);
                }
                if self.state.trashed {
                    return Err(Reason::Trashed);
                }
                self.state.trashed = true;
                Ok(Applied::default())
            }
            Op::RestoreCascade => {
                if self.purged {
                    return Err(Reason::NotFound);
                }
                if !self.state.trashed {
                    return Err(Reason::NotTrashed);
                }
                self.state.trashed = false;
                Ok(Applied::default())
            }
            Op::PurgeQuiz { quiz } => {
                let q = self.quiz(*quiz)?;
                if self.state.trashed {
                    return Err(Reason::Trashed);
                }
                if q.state.active {
                    return Err(Reason::NotCleared);
                }
                self.quizzes.remove(quiz);
                self.purged_quizzes.insert(*quiz);
                Ok(Applied::default())
            }
            Op::PurgeCascade => {
                if self.purged {
                    return Err(Reason::NotFound);
                }
                if !self.state.trashed {
                    return Err(Reason::NotTrashed);
                }
                self.purged = true;
                let ids: Vec<Uuid> = self.quizzes.keys().copied().collect();
                self.purged_quizzes.extend(ids);
                self.quizzes.clear();
                Ok(Applied::default())
            }
            Op::SetCascadeOptions { segment_size, progression, require_alphabetical } => {
                if self.purged {
                    return Err(Reason::NotFound);
                }
                if self.state.trashed {
                    return Err(Reason::Trashed);
                }
                let size = segment_size.map(|s| rules::check_segment_size(s, self.max_quiz_questions)).transpose()?;
                let o = &mut self.state.opts;
                if let Some(s) = size {
                    o.segment_size = s;
                }
                if let Some(p) = progression {
                    o.progression = *p;
                }
                if let Some(a) = require_alphabetical {
                    o.require_alphabetical = *a;
                }
                Ok(Applied::default())
            }
            Op::SetQuizOptions { quiz, segment_size, progression, require_alphabetical } => {
                let q = self.live(*quiz)?;
                let size = segment_size.map(|s| rules::check_segment_size(s, self.max_quiz_questions)).transpose()?;
                rules::check_quiz_option_fields(&q.state, progression.is_some(), segment_size.is_some())?;
                let o = &mut self.quizzes.get_mut(quiz).unwrap().state.opts;
                if let Some(s) = size {
                    o.segment_size = s;
                }
                if let Some(p) = progression {
                    o.progression = *p;
                }
                if let Some(a) = require_alphabetical {
                    o.require_alphabetical = *a;
                }
                Ok(Applied::default())
            }
        }
    }

    fn reset_in_place(&mut self, quiz: Uuid, seed: u64, out: &mut Applied) {
        let q = self.quizzes.get_mut(&quiz).unwrap();
        q.reset(seed);
        out.new_quiz_question_count = Some(q.state.question_count);
        out.new_quiz_questions_hash = Some(questions_hash(&q.questions));
    }
}
