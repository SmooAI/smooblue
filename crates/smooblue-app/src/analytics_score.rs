//! Account analytics — follower clout scoring (pure, no IO).
//!
//! Turns a [`crate::analytics::FollowerStat`]'s raw signals (the
//! follower's own reach, how often they've engaged with us, whether the
//! follow is mutual, and how long they've followed us) into the four
//! component scores plus the blended `composite_score` the ranked-cut
//! views sort on.
//!
//! All functions here are pure (modulo `Utc::now()` for the recency /
//! tenure decays, which read the wall clock) so they're directly
//! unit-testable. The store persists `reach_score` / `engagement_score`
//! / `mutual_bonus` / `composite_score`; `tenure_bonus` is recomputed at
//! read time and folded into the composite (it is **not** a column).
//!
//! ## Score model
//!
//! `composite = REACH_WEIGHT·reach + ENGAGEMENT_WEIGHT·engagement
//!            + mutual_bonus + tenure_bonus`, clamped to `1.0`.
//!
//! `reach` and `engagement` are normalized `0.0..=1.0` factors that the
//! composite multiplies by their weight. `mutual_bonus` and
//! `tenure_bonus` are returned **already weighted** (so they max out at
//! `MUTUAL_WEIGHT` / `TENURE_WEIGHT`). The four weights sum to `1.0`, so
//! an all-max follower scores exactly `1.0`.

use chrono::{DateTime, Utc};

// ───────────────────────────── weights ─────────────────────────────

/// Weight of the follower's own reach in the composite.
pub const REACH_WEIGHT: f64 = 0.30;
/// Weight of how much the follower engages with us.
pub const ENGAGEMENT_WEIGHT: f64 = 0.40;
/// Flat bonus (already weighted) for a mutual follow.
pub const MUTUAL_WEIGHT: f64 = 0.15;
/// Max bonus (already weighted) for a long-tenured follower.
pub const TENURE_WEIGHT: f64 = 0.15;

// ─────────────────────────── tuning knobs ──────────────────────────

/// `log10(followers)` value that maps to a full reach score of `1.0`
/// (`10^6` followers). `log10(100)/6 ≈ 0.33`, `log10(10_000)/6 ≈ 0.67`.
pub const MAX_REACH_SCORE: f64 = 6.0;
/// Engagement count that saturates the count component at `1.0`.
pub const ENGAGEMENT_SATURATION: f64 = 50.0;
/// Half-life (days) of the engagement recency decay — a 30-day-old last
/// engagement is worth half a same-day one.
pub const ENGAGEMENT_HALFLIFE_DAYS: f64 = 30.0;
/// Engagements older than this window contribute nothing.
pub const ENGAGEMENT_RECENCY_WINDOW_DAYS: f64 = 90.0;
/// Tenure (days) at which the tenure bonus saturates at `TENURE_WEIGHT`.
pub const FOLLOWER_TENURE_BOOST_DAYS: f64 = 180.0;
/// Floor multiplier so an in-window engagement never decays below
/// `MIN_ENGAGEMENT_WEIGHT × count_score`.
pub const MIN_ENGAGEMENT_WEIGHT: f64 = 0.1;

// ─────────────────────────── component fns ─────────────────────────

/// Normalized reach `0.0..=1.0` from the follower's follower count, on a
/// `log10` curve capped at `10^MAX_REACH_SCORE`. `0` followers → `0.0`.
pub fn compute_reach_score(followers_count: i64) -> f64 {
    if followers_count <= 0 {
        return 0.0;
    }
    ((followers_count as f64).log10() / MAX_REACH_SCORE).clamp(0.0, 1.0)
}

/// Normalized engagement `0.0..=1.0` from how many times the follower has
/// engaged and how recently. `None` last-engagement → `0.0`. Engagements
/// older than [`ENGAGEMENT_RECENCY_WINDOW_DAYS`] → `0.0`. Within the
/// window the count component (saturating at [`ENGAGEMENT_SATURATION`])
/// is decayed by a [`ENGAGEMENT_HALFLIFE_DAYS`] half-life, floored at
/// `MIN_ENGAGEMENT_WEIGHT × count_score`.
pub fn compute_engagement_score(
    engagement_count: i64,
    last_engagement_ts: Option<DateTime<Utc>>,
) -> f64 {
    let Some(last) = last_engagement_ts else {
        return 0.0;
    };
    if engagement_count <= 0 {
        return 0.0;
    }
    let age_days = (Utc::now() - last).num_seconds().max(0) as f64 / 86_400.0;
    if age_days > ENGAGEMENT_RECENCY_WINDOW_DAYS {
        return 0.0;
    }
    let count_score = ((engagement_count as f64) / ENGAGEMENT_SATURATION).clamp(0.0, 1.0);
    let recency = 0.5_f64.powf(age_days / ENGAGEMENT_HALFLIFE_DAYS);
    (count_score * recency).max(MIN_ENGAGEMENT_WEIGHT * count_score)
}

/// Already-weighted mutual-follow bonus: [`MUTUAL_WEIGHT`] or `0.0`.
pub fn compute_mutual_bonus(mutual: bool) -> f64 {
    if mutual {
        MUTUAL_WEIGHT
    } else {
        0.0
    }
}

/// Already-weighted tenure bonus, ramping linearly from `0.0` at the
/// follow instant up to [`TENURE_WEIGHT`] at [`FOLLOWER_TENURE_BOOST_DAYS`].
pub fn compute_tenure_bonus(first_followed_ts: DateTime<Utc>) -> f64 {
    let age_days = (Utc::now() - first_followed_ts).num_seconds().max(0) as f64 / 86_400.0;
    let frac = (age_days / FOLLOWER_TENURE_BOOST_DAYS).clamp(0.0, 1.0);
    frac * TENURE_WEIGHT
}

/// Blend the four components into the composite, clamped to `1.0`.
/// `reach` / `engagement` are normalized factors (weighted here);
/// `mutual_bonus` / `tenure_bonus` arrive pre-weighted.
pub fn compute_composite_score(
    reach: f64,
    engagement: f64,
    mutual_bonus: f64,
    tenure_bonus: f64,
) -> f64 {
    (REACH_WEIGHT * reach + ENGAGEMENT_WEIGHT * engagement + mutual_bonus + tenure_bonus)
        .clamp(0.0, 1.0)
}

/// Fill `reach_score` / `engagement_score` / `mutual_bonus` /
/// `tenure_bonus` / `composite_score` on a [`crate::analytics::FollowerStat`]
/// in place from its raw signals.
pub fn rescore(stat: &mut crate::analytics::FollowerStat) {
    stat.reach_score = compute_reach_score(stat.followers_count);
    stat.engagement_score =
        compute_engagement_score(stat.engagement_count, stat.last_engagement_ts);
    stat.mutual_bonus = compute_mutual_bonus(stat.mutual);
    stat.tenure_bonus = compute_tenure_bonus(stat.first_followed_ts);
    stat.composite_score = compute_composite_score(
        stat.reach_score,
        stat.engagement_score,
        stat.mutual_bonus,
        stat.tenure_bonus,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 0.02, "{a} not ≈ {b}");
    }

    #[test]
    fn reach_score_log_curve_and_cap() {
        assert_eq!(compute_reach_score(0), 0.0);
        approx(compute_reach_score(100), 0.33);
        approx(compute_reach_score(10_000), 0.67);
        assert_eq!(compute_reach_score(1_000_000), 1.0);
        // Above the cap still clamps at 1.0.
        assert_eq!(compute_reach_score(50_000_000), 1.0);
    }

    #[test]
    fn engagement_none_and_zero_are_zero() {
        assert_eq!(compute_engagement_score(5, None), 0.0);
        assert_eq!(compute_engagement_score(0, Some(Utc::now())), 0.0);
    }

    #[test]
    fn engagement_fresh_saturated_is_one() {
        approx(compute_engagement_score(50, Some(Utc::now())), 1.0);
    }

    #[test]
    fn engagement_outside_window_is_zero() {
        let old = Utc::now() - Duration::days(100);
        assert_eq!(compute_engagement_score(50, Some(old)), 0.0);
    }

    #[test]
    fn engagement_halflife_halves_at_30_days() {
        let now = compute_engagement_score(50, Some(Utc::now()));
        let d30 = compute_engagement_score(50, Some(Utc::now() - Duration::days(30)));
        approx(d30, now / 2.0);
    }

    #[test]
    fn engagement_floor_applies_within_window() {
        // 89 days old, far past the halflife (~0.13×) but still inside the
        // window → at least MIN_ENGAGEMENT_WEIGHT × count_score (count=50
        // → count_score 1.0 → floor 0.1).
        let s = compute_engagement_score(50, Some(Utc::now() - Duration::days(89)));
        assert!(s >= MIN_ENGAGEMENT_WEIGHT, "{s} below floor");
    }

    #[test]
    fn tenure_bonus_ramps() {
        assert_eq!(compute_tenure_bonus(Utc::now()), 0.0);
        approx(
            compute_tenure_bonus(Utc::now() - Duration::days(90)),
            TENURE_WEIGHT / 2.0,
        );
        assert_eq!(
            compute_tenure_bonus(Utc::now() - Duration::days(180)),
            TENURE_WEIGHT
        );
        // Past the boost window still caps at TENURE_WEIGHT.
        assert_eq!(
            compute_tenure_bonus(Utc::now() - Duration::days(900)),
            TENURE_WEIGHT
        );
    }

    #[test]
    fn mutual_bonus_is_flat() {
        assert_eq!(compute_mutual_bonus(true), MUTUAL_WEIGHT);
        assert_eq!(compute_mutual_bonus(false), 0.0);
    }

    #[test]
    fn composite_all_max_is_one() {
        let c = compute_composite_score(1.0, 1.0, MUTUAL_WEIGHT, TENURE_WEIGHT);
        approx(c, 1.0);
        // Weight identity: the four weights sum to 1.0.
        approx(
            REACH_WEIGHT + ENGAGEMENT_WEIGHT + MUTUAL_WEIGHT + TENURE_WEIGHT,
            1.0,
        );
    }

    #[test]
    fn composite_clamps_at_one() {
        let c = compute_composite_score(2.0, 2.0, MUTUAL_WEIGHT, TENURE_WEIGHT);
        assert_eq!(c, 1.0);
    }

    #[test]
    fn rescore_fills_components() {
        let mut stat = crate::analytics::FollowerStat {
            follower_did: "did:plc:x".into(),
            follower_handle: "x.bsky.social".into(),
            follower_display_name: None,
            follower_avatar: None,
            followers_count: 1_000_000,
            following_count: 10,
            engagement_count: 50,
            last_engagement_ts: Some(Utc::now()),
            mutual: true,
            first_followed_ts: Utc::now() - Duration::days(180),
            reach_score: 0.0,
            engagement_score: 0.0,
            mutual_bonus: 0.0,
            tenure_bonus: 0.0,
            composite_score: 0.0,
        };
        rescore(&mut stat);
        assert_eq!(stat.reach_score, 1.0);
        approx(stat.engagement_score, 1.0);
        assert_eq!(stat.mutual_bonus, MUTUAL_WEIGHT);
        assert_eq!(stat.tenure_bonus, TENURE_WEIGHT);
        approx(stat.composite_score, 1.0);
    }
}
