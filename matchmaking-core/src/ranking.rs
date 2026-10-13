//! Ranking v1: checking an ordering against what actually happened.
//!
//! A score already exists for every recommended pair, and so does the move that ended the process. This module puts
//! the two side by side: given a set of recorded decisions, which of two orderings agrees better with the outcomes?
//!
//! Two orderings are on trial, and only these two:
//! * **baseline** -- the confidence-adjusted `ranking_score`, which is what decides the order of a search today. The
//!   adjustment pulls a score resting on thin evidence towards a neutral prior, so a well-documented pair can outrank
//!   a better-scoring but poorly-documented one.
//! * **candidate** -- the raw coverage-weighted `overall`, before that adjustment. If the adjustment is worth its
//!   complexity then it must beat the raw number on recorded outcomes; if it does not, it is costing ordering quality
//!   for nothing.
//!
//! The verdict is **pairwise concordance**: of the pairs whose outcomes differ, in what share does the higher-scoring
//! pair hold the better outcome? Ties are neither wins nor losses, so they are excluded from the rate and reported as
//! their own count. A set where every pair ties has no rate at all -- reported as `None`, never as `0.0` and never as a
//! `NaN`, because "no comparisons" and "compared and always wrong" are different findings.
//!
//! This is the whole of ranking v1's comparison. It reads recorded history; it fits nothing, it stores nothing, it
//! changes no search, and it is not a trained model. Nothing here is used to order a result list.

use serde::Serialize;

/// Which way a recorded `Outcome` counts once the question is "did this pair progress?".
///
/// The mapping is deliberately coarser than the nine outcomes, because the comparison needs a direction, not a stage:
/// a pair that met twice is not more progressed than a pair that met once for the purposes of *which of these two
/// candidates worked out*, and a pair that reached "interested" did better than one that was turned down. Making a
/// finer distinction would be inventing a ranking of outcomes that the product plan does not define.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Progress {
    Progressed,
    NotProgressed,
    /// The outcome carries no information about whether the pair progressed -- and never will, unlike the seven stages
    /// below it, which may still move. Excluded from the comparison rather than guessed into either side.
    Excluded,
}

/// The classification rule, in one place, so the command that reports it and the code that applies it cannot drift.
/// Unrecognised text is `Excluded` for the same reason `unknown` is: an outcome this version does not understand is
/// absence of information, and reading it as a failure to progress would invent a negative.
pub fn classify(outcome: &str) -> Progress {
    match outcome {
        "interested" | "first_conversation" | "first_meeting" | "continued" | "relationship_formed" | "married" => {
            Progress::Progressed
        }
        "not_interested" | "stopped" => Progress::NotProgressed,
        _ => Progress::Excluded,
    }
}

/// One analysed match: the two scores the snapshot recorded, and what became of it.
///
/// A score of `None` is a match that has no stored snapshot, or a snapshot whose card never produced that number;
/// either way there is nothing to place, and the row is skipped for that ordering rather than filled in with a zero.
#[derive(Debug, Clone, PartialEq)]
pub struct ComparisonRow {
    pub overall: Option<f64>,
    pub ranking_score: Option<f64>,
    pub progressed: bool,
}

/// How one ordering did over the same set of rows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OrderingResult {
    /// Pairs that could be compared: both rows scored, and their outcomes differed.
    pub comparable_pairs: usize,
    pub concordant: usize,
    pub discordant: usize,
    /// Pairs whose outcomes were the same, so neither ordering could have got them in or out of order.
    pub tied: usize,
    /// `concordant / (concordant + discordant)`, or `None` when nothing was comparable. A tie is in neither term.
    pub concordance: Option<f64>,
    /// Rows this ordering could place at all (non-missing score), whether or not their outcome was usable.
    pub usable_rows: usize,
    /// Of those rows, how many progressed.
    pub progressed: usize,
    pub not_progressed: usize,
    /// R-precision: of the top `progressed` rows by this score, how many actually progressed. Compared against the
    /// `progressed` rows one would expect by chance, the same count.
    pub top_k: usize,
    pub top_k_rate: Option<f64>,
}

/// The comparison over the whole recorded set, for both orderings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RankingComparison {
    /// Matches analysed (a match with no outcome has no label and is not one of these).
    pub total_rows: usize,
    pub progressed: usize,
    pub not_progressed: usize,
    /// Matches whose labels are known, before a missing score can drop them from one ordering.
    pub usable_rows: usize,
    /// The confidence-adjusted `ranking_score`, which orders searches today.
    pub baseline: OrderingResult,
    /// The raw coverage-weighted `overall`, before the confidence adjustment.
    pub candidate: OrderingResult,
}

/// Compare two orderings against the recorded outcomes. Pure: no database, no clock, no model.
pub fn compare(rows: &[ComparisonRow]) -> RankingComparison {
    RankingComparison {
        total_rows: rows.len(),
        progressed: rows.iter().filter(|r| r.progressed).count(),
        not_progressed: rows.iter().filter(|r| !r.progressed).count(),
        usable_rows: rows.len(),
        baseline: against(rows, |r| r.ranking_score),
        candidate: against(rows, |r| r.overall),
    }
}

/// Score every pair of rows (`i < j`) once, as judged by one accessor. A pair is skipped when either row has no score
/// for this ordering -- there is no way to say which "scored higher" -- and when the two outcomes agree, since a tie
/// cannot favour either ordering and counting it as a win would flatter both equally.
fn against(rows: &[ComparisonRow], score_of: impl Fn(&ComparisonRow) -> Option<f64>) -> OrderingResult {
    let mut comparable_pairs = 0usize;
    let mut concordant = 0usize;
    let mut discordant = 0usize;
    let mut tied = 0usize;
    for i in 0..rows.len() {
        for j in (i + 1)..rows.len() {
            let (a, b) = (&rows[i], &rows[j]);
            let (Some(sa), Some(sb)) = (score_of(a), score_of(b)) else { continue };
            if a.progressed == b.progressed {
                tied += 1;
                continue;
            }
            comparable_pairs += 1;
            // The higher score should belong to the pair that progressed. Ties in the *score* are not possible here
            // only because the two rows are distinct; equal scores fall through as discordant, which is the honest
            // reading -- an ordering that cannot separate them did not get the order right.
            if (sa > sb) == a.progressed {
                concordant += 1;
            } else {
                discordant += 1;
            }
        }
    }

    let scored: Vec<(&ComparisonRow, f64)> = rows.iter().filter_map(|r| score_of(r).map(|s| (r, s))).collect();
    let usable_rows = scored.len();
    let progressed = scored.iter().filter(|(r, _)| r.progressed).count();
    let not_progressed = scored.iter().filter(|(r, _)| !r.progressed).count();

    let concordance = if concordant + discordant > 0 {
        Some(concordant as f64 / (concordant + discordant) as f64)
    } else {
        None
    };

    // R-precision. `k` is the number of progressed rows, so the chance-level hit count is that same number: this
    // measures the ordering rather than the quantity of good pairs in the set, which a plain precision-at-k would not.
    let mut order: Vec<(&ComparisonRow, f64)> = scored.clone();
    order.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
    let k = progressed;
    let top_k = order.iter().take(k).filter(|(r, _)| r.progressed).count();
    let top_k_rate = if k > 0 { Some(top_k as f64 / k as f64) } else { None };

    OrderingResult { comparable_pairs, concordant, discordant, tied, concordance, usable_rows, progressed, not_progressed, top_k, top_k_rate }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(overall: Option<f64>, ranking: Option<f64>, progressed: bool) -> ComparisonRow {
        ComparisonRow { overall, ranking_score: ranking, progressed }
    }

    #[test]
    fn all_nine_outcomes_are_classified_and_an_unknown_text_is_excluded() {
        for progressed in ["interested", "first_conversation", "first_meeting", "continued", "relationship_formed", "married"] {
            assert_eq!(classify(progressed), Progress::Progressed, "{progressed}");
        }
        for not_progressed in ["not_interested", "stopped"] {
            assert_eq!(classify(not_progressed), Progress::NotProgressed, "{not_progressed}");
        }
        assert_eq!(classify("unknown"), Progress::Excluded);
        // Text from a version that wrote something else is not a failure to progress.
        assert_eq!(classify(""), Progress::Excluded);
        assert_eq!(classify("interested "), Progress::Excluded);
        assert_eq!(classify("Introduced"), Progress::Excluded);
    }

    #[test]
    fn concordance_counts_agreements_against_disagreements() {
        // Three progressed pairs with the higher score and one that did not: 3 concordant, 1 discordant.
        let rows = vec![
            row(Some(80.0), Some(70.0), true),
            row(Some(70.0), Some(60.0), true),
            row(Some(60.0), Some(50.0), true),
            row(Some(30.0), Some(90.0), false),
        ];
        let c = compare(&rows);
        assert_eq!(c.total_rows, 4);
        assert_eq!((c.progressed, c.not_progressed), (3, 1));
        assert_eq!(c.candidate.comparable_pairs, 3);
        assert_eq!((c.candidate.concordant, c.candidate.discordant), (3, 0));
        assert_eq!(c.candidate.concordance, Some(1.0));
        // The raw overall gets all three right; the confidence-adjusted score puts the two progressed rows far below
        // the 90 of the one that failed, so it disagrees with the outcome on every one of the three.
        assert_eq!(c.baseline.comparable_pairs, 3);
        assert_eq!((c.baseline.concordant, c.baseline.discordant), (0, 3));
        assert_eq!(c.baseline.concordance, Some(0.0));
        assert_eq!(c.candidate.tied, 3, "the pairs inside each outcome group are ties, not wins");
    }

    #[test]
    fn pairs_with_the_same_outcome_are_tied_and_stay_out_of_the_rate() {
        // Every row progressed: every pair shares an outcome, so no ordering can be judged at all.
        let all_progressed = vec![row(Some(90.0), Some(90.0), true), row(Some(80.0), Some(80.0), true), row(Some(20.0), Some(20.0), true)];
        let c = compare(&all_progressed);
        assert_eq!((c.candidate.tied, c.baseline.tied), (3, 3));
        assert_eq!(c.candidate.concordant + c.candidate.discordant, 0);
        assert_eq!(c.candidate.concordance, None, "no comparison is not a rate of zero");
        assert_eq!(c.baseline.concordance, None);
        // The rows are still placed on the scale and still counted as progressed and not.
        assert_eq!((c.candidate.usable_rows, c.candidate.progressed, c.candidate.not_progressed), (3, 3, 0));

        // In a mixed set the same-outcome pairs are reported as ties rather than folded into either side.
        let mixed = vec![
            row(Some(90.0), Some(90.0), true),
            row(Some(80.0), Some(80.0), true),
            row(Some(20.0), Some(20.0), false),
            row(Some(10.0), Some(10.0), false),
        ];
        let m = compare(&mixed);
        assert_eq!(m.candidate.tied, 2);
        assert_eq!((m.candidate.concordant, m.candidate.discordant), (4, 0));
        assert_eq!(m.candidate.comparable_pairs + m.candidate.tied, 6, "every pair is compared or tied");
    }

    #[test]
    fn a_set_with_no_progressed_rows_has_no_r_precision_rather_than_zero() {
        let rows = vec![row(Some(40.0), Some(40.0), false), row(Some(30.0), Some(30.0), false)];
        let c = compare(&rows);
        assert_eq!(c.progressed, 0);
        assert_eq!(c.candidate.top_k, 0);
        assert_eq!(c.candidate.top_k_rate, None);
        assert_eq!(c.candidate.concordance, None, "one class only: nothing to order");
    }

    #[test]
    fn an_empty_set_compares_to_nothing_rather_than_to_zero() {
        let c = compare(&[]);
        assert_eq!(c.total_rows, 0);
        assert_eq!((c.progressed, c.not_progressed), (0, 0));
        assert_eq!(c.candidate.concordance, None);
        assert_eq!(c.baseline.concordance, None);
        assert_eq!(c.candidate.comparable_pairs, 0);
        assert_eq!(c.candidate.top_k_rate, None);
    }

    #[test]
    fn a_row_with_no_stored_score_is_skipped_by_that_ordering_and_kept_by_the_other() {
        let rows = vec![
            row(None, Some(70.0), true),
            row(Some(30.0), Some(60.0), false),
            row(Some(80.0), Some(50.0), true),
        ];
        let c = compare(&rows);
        assert_eq!(c.total_rows, 3);
        // The raw overall can only place the two rows that have one.
        assert_eq!((c.candidate.usable_rows, c.candidate.progressed, c.candidate.not_progressed), (2, 1, 1));
        assert_eq!(c.candidate.comparable_pairs, 1);
        assert_eq!(c.candidate.concordance, Some(1.0));
        // The adjusted score can place all three.
        assert_eq!((c.baseline.usable_rows, c.baseline.progressed), (3, 2));
        assert_eq!(c.baseline.comparable_pairs, 2);
    }

    /// The point of the whole harness: a set built so that one ordering tracks the outcomes and the other does not
    /// must produce a better concordance for the former. If these two did not invert, the comparison would be
    /// measuring nothing.
    #[test]
    fn the_better_ordering_scores_higher_on_a_crafted_set_and_the_reverse_input_inverts_it() {
        // Progressers carry a high raw overall but a low adjusted score (thin evidence); non-progressers the reverse.
        let raw_tracks = vec![
            row(Some(90.0), Some(40.0), true),
            row(Some(80.0), Some(30.0), true),
            row(Some(20.0), Some(90.0), false),
            row(Some(10.0), Some(80.0), false),
        ];
        let c = compare(&raw_tracks);
        assert_eq!(c.candidate.concordance, Some(1.0), "raw overall separates the two groups");
        assert_eq!(c.baseline.concordance, Some(0.0), "the adjusted score inverts both");
        assert!(c.candidate.concordance > c.baseline.concordance);
        assert_eq!((c.candidate.top_k, c.baseline.top_k), (2, 0));

        // The same shape with the two score columns swapped: now the adjusted score is the one that tracks.
        let adjusted_tracks = vec![
            row(Some(40.0), Some(90.0), true),
            row(Some(30.0), Some(80.0), true),
            row(Some(90.0), Some(20.0), false),
            row(Some(80.0), Some(10.0), false),
        ];
        let flipped = compare(&adjusted_tracks);
        assert_eq!(flipped.baseline.concordance, Some(1.0));
        assert_eq!(flipped.candidate.concordance, Some(0.0));
        assert!(flipped.baseline.concordance > flipped.candidate.concordance);
    }

    #[test]
    fn identical_orderings_produce_identical_results() {
        let rows = vec![row(Some(70.0), Some(70.0), true), row(Some(60.0), Some(60.0), true), row(Some(20.0), Some(20.0), false)];
        let c = compare(&rows);
        assert_eq!(c.candidate, c.baseline);
    }

    #[test]
    fn equal_scores_do_not_count_as_agreement() {
        // Same score, opposite outcomes: the ordering put them level, so it did not get the pair in order.
        let rows = vec![row(Some(50.0), Some(50.0), true), row(Some(50.0), Some(50.0), false)];
        let c = compare(&rows);
        assert_eq!((c.candidate.concordant, c.candidate.discordant), (0, 1));
        assert_eq!(c.candidate.concordance, Some(0.0));
    }
}
