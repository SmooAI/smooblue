//! Account-analytics dashboard view (pearl account-analytics).
//!
//! Pure rsx! → inline `<svg>`. No JS, no Chart.js — every chart is an
//! SVG built from data the [`crate::analytics`] store already aggregated
//! into [`AnalyticsData`] off the render thread (see `column.rs`'s
//! Analytics fetch arm, which runs `build_analytics_data` on a blocking
//! task).
//!
//! Three chart helpers (each a `#[component]`):
//! - [`LineChart`] — followers-vs-following cumulative growth (one
//!   polyline per series, optional filled area underneath).
//! - [`BarChart`] — posts-per-day.
//! - [`CadenceHeatmap`] — a 7×24 (weekday × hour) posting-rhythm grid.
//!
//! Plus two ranked lists ([`TopFollowersList`], [`TopPostsList`]) that
//! read the scoring system of record's `FollowerStat` / `PostMetric`
//! store types directly — no `ActorProfile` / `PostView` hydration in v1.
//!
//! The SVG **coordinate math** lives in the free functions [`line_x`],
//! [`line_y`], and [`bar_height`] so it's unit-testable without a
//! renderer (see the `tests` module). The components only string the
//! coordinates into attributes.

use crate::analytics::{FollowerStat, MetricSnapshot, PostMetric};
use dioxus::prelude::*;

/// Inner padding (px) between a chart's drawing area and its viewBox
/// edge — leaves room for the stroke width and the baseline so the
/// extreme points aren't clipped.
const CHART_PADDING: f64 = 8.0;
/// Growth line chart viewBox dimensions (the SVG scales to the column
/// width via `width: 100%`; the viewBox is the coordinate space the
/// math below works in).
const LINE_W: f64 = 320.0;
const LINE_H: f64 = 140.0;

// ───────────────────────── view DTO ────────────────────────────────

/// Everything the [`AnalyticsView`] renders, rolled up by
/// [`crate::analytics::build_analytics_data`]. All fields are plain
/// owned data so the column can clone it cheaply into the render arm.
#[derive(Clone, PartialEq, Default)]
pub struct AnalyticsData {
    /// `YYYY-MM` label per growth sample (full account history, monthly).
    pub growth_labels: Vec<String>,
    /// Cumulative followers per month, oldest → newest.
    pub followers_over_time: Vec<f64>,
    /// Cumulative following per month, oldest → newest.
    pub following_over_time: Vec<f64>,
    /// One bar per calendar day (count of own posts).
    pub posts_per_day: Vec<BarDatum>,
    /// `[7][24]` (weekday × hour) posting cadence, normalized `0.0..=1.0`.
    pub cadence: Vec<Vec<f64>>,
    /// Your most engaged fans (composite-ranked), `list_top_fans`.
    pub top_fans: Vec<FollowerStat>,
    /// Your highest-reach mutuals, `list_mutuals_by_reach`.
    pub top_mutuals: Vec<FollowerStat>,
    /// Top own posts by like count.
    pub top_posts: Vec<PostMetric>,
    /// [`crate::analytics::BackfillPhase::rank`] of the backfill machine
    /// — drives per-card "still collecting" vs "done + empty" states.
    pub backfill_phase_rank: u8,
    /// `true` once the one-time backfill has fully completed.
    pub backfill_complete: bool,
}

/// Current displayed counts plus 7-day and 30-day deltas, surfaced in the
/// pop-out's summary header. Deltas are computed by the pure
/// [`crate::analytics::metric_deltas`] helper over the snapshot history,
/// so this carries plain owned scalars.
#[derive(Clone, PartialEq, Default)]
pub struct SummaryStats {
    pub current_followers: i64,
    pub current_following: i64,
    pub current_posts: i64,
    pub delta_followers_7d: i64,
    pub delta_following_7d: i64,
    pub delta_posts_7d: i64,
    pub delta_followers_30d: i64,
    pub delta_following_30d: i64,
    pub delta_posts_30d: i64,
}

/// One month's summed first-party engagement across own posts. Ascending
/// by `month` (`"YYYY-MM"`). Built by the pure
/// [`crate::analytics::bucket_engagement_monthly`] helper.
#[derive(Clone, PartialEq, Default)]
pub struct EngagementMetrics {
    /// `"YYYY-MM"`.
    pub month: String,
    pub likes: i64,
    pub reposts: i64,
    pub replies: i64,
    pub quotes: i64,
}

/// Superset of [`AnalyticsData`] the pop-out "deep dive" renders. The
/// glanceable base fields are reused verbatim from
/// [`crate::analytics::build_analytics_data`]; the deep-dive additions
/// (summary deltas, four follower lenses, monthly engagement, true
/// per-metric top-posts) come from extra store reads + pure helpers. All
/// fields are plain owned data so the modal can clone it cheaply.
#[derive(Clone, PartialEq, Default)]
pub struct ExpandedAnalyticsData {
    // ── superset of AnalyticsData (glanceable base, reused verbatim) ──
    /// `YYYY-MM` label per growth sample (full account history, monthly).
    pub growth_labels: Vec<String>,
    pub followers_over_time: Vec<f64>,
    pub following_over_time: Vec<f64>,
    /// Snapshot-sourced followers overlay, resampled onto `growth_labels`
    /// (index-aligned, same length).
    pub net_followers_by_month: Vec<f64>,
    pub posts_per_day: Vec<BarDatum>,
    pub cadence: Vec<Vec<f64>>,
    pub backfill_phase_rank: u8,
    pub backfill_complete: bool,

    // ── deep-dive additions ──
    pub summary: SummaryStats,
    pub top_fans: Vec<FollowerStat>,
    pub high_clout_not_mutual: Vec<FollowerStat>,
    pub mutuals_by_reach: Vec<FollowerStat>,
    pub lurkers_by_clout: Vec<FollowerStat>,
    pub engagement_monthly: Vec<EngagementMetrics>,
    pub top_posts_by_likes: Vec<PostMetric>,
    pub top_posts_by_reposts: Vec<PostMetric>,
    pub top_posts_by_replies: Vec<PostMetric>,
    /// Like-ranked top posts across all of history.
    pub top_posts_all_time: Vec<PostMetric>,
    /// Like-ranked top posts from the last 365 days.
    pub top_posts_last_year: Vec<PostMetric>,
    /// Like-ranked top posts from the last 30 days.
    pub top_posts_last_month: Vec<PostMetric>,
    /// Number of daily snapshots captured — gates the exact net-followers
    /// overlay (needs `>= 2` to draw a meaningful line).
    pub snapshot_count: usize,
}

/// Backfill-phase ranks (mirror of [`crate::analytics::BackfillPhase::rank`])
/// used to decide whether a card's data source has been populated yet.
pub const PHASE_FOLLOWING: u8 = 2;
pub const PHASE_FOLLOWERS: u8 = 3;
pub const PHASE_ENGAGEMENT: u8 = 4;

/// One labelled bar. Shared with the store aggregator so it can build
/// the per-day series without depending back on the view internals.
#[derive(Clone, PartialEq)]
pub struct BarDatum {
    pub label: String,
    pub value: f64,
}

/// A named line series for [`LineChart`]. Private — the view builds
/// these from [`AnalyticsData`]'s parallel vectors.
#[derive(Clone, PartialEq)]
struct ChartSeries {
    label: String,
    values: Vec<f64>,
    /// CSS class controlling the stroke / fill color.
    class: String,
}

// ──────────────────── pure coordinate math ─────────────────────────

/// X coordinate of the `i`-th of `n` evenly-spaced points. The first
/// point sits at `padding`, the last at `width - padding`. With a
/// single point (or none) there's no span to divide by, so it pins to
/// the left padding — no divide-by-zero on `n - 1`.
pub fn line_x(i: usize, n: usize, width: f64, padding: f64) -> f64 {
    if n <= 1 {
        return padding;
    }
    padding + (i as f64 / (n - 1) as f64) * (width - 2.0 * padding)
}

/// Y coordinate for `value` on a `[min, max]` scale. `value == max`
/// maps to `padding` (top), `value == min` maps to `height - padding`
/// (baseline) — SVG y grows downward. A zero-width range (all values
/// equal) can't be normalized, so it rests on the baseline rather than
/// dividing by zero.
pub fn line_y(value: f64, min: f64, max: f64, height: f64, padding: f64) -> f64 {
    let range = max - min;
    if range <= 0.0 {
        return height - padding;
    }
    height - padding - ((value - min) / range) * (height - 2.0 * padding)
}

/// Pixel height of a bar whose `value` is drawn against `max` within a
/// `chart_h`-tall plotting area. Guards a zero/negative `max` (empty
/// data) so the bar collapses to nothing instead of `NaN`/`inf`.
pub fn bar_height(value: f64, max: f64, chart_h: f64) -> f64 {
    if max <= 0.0 || value <= 0.0 {
        return 0.0;
    }
    (value / max) * chart_h
}

/// Evenly-spaced y-axis tick *values* across `[min, max]` — `count`
/// inclusive samples (e.g. `count = 3` → `[min, mid, max]`). A flat range
/// (`min == max`, or an inverted/degenerate `count`) collapses to a single
/// `[min]` tick so the axis never emits `NaN` or duplicate labels.
///
/// Pure — no IO — so it's directly unit-testable.
pub fn y_axis_ticks(min: f64, max: f64, count: usize) -> Vec<f64> {
    if max <= min || count < 2 {
        return vec![min];
    }
    let step = (max - min) / (count - 1) as f64;
    (0..count).map(|i| min + step * i as f64).collect()
}

/// Indices into a `len`-length label vector to actually render — about
/// `target` evenly-spaced labels, always anchored on the first and last
/// index so the axis reads from the real start/end. Returns every index
/// when `len <= target` (nothing to subsample) and `[]` for an empty
/// label set. Indices are unique and ascending.
///
/// Pure — no IO — so it's directly unit-testable.
pub fn x_axis_label_indices(len: usize, target: usize) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    if target <= 1 {
        return vec![0];
    }
    if len <= target {
        return (0..len).collect();
    }
    // Spread `target` picks across [0, len-1] inclusive, deduping any
    // rounding collisions so two picks never land on the same index.
    let last = (len - 1) as f64;
    let mut out: Vec<usize> = Vec::with_capacity(target);
    for i in 0..target {
        let idx = (i as f64 / (target - 1) as f64 * last).round() as usize;
        if out.last() != Some(&idx) {
            out.push(idx);
        }
    }
    out
}

/// Build the three engagement-over-time line series (likes / reposts /
/// replies) from the monthly engagement buckets, index-aligned to the
/// bucket order (ascending by month). Quotes are carried in the DTO but
/// not drawn in v1 — three lines stay readable on a column-width chart.
fn engagement_series(metrics: &[EngagementMetrics]) -> Vec<ChartSeries> {
    vec![
        ChartSeries {
            label: "Likes".into(),
            values: metrics.iter().map(|m| m.likes as f64).collect(),
            class: "analytics__line--likes".into(),
        },
        ChartSeries {
            label: "Reposts".into(),
            values: metrics.iter().map(|m| m.reposts as f64).collect(),
            class: "analytics__line--reposts".into(),
        },
        ChartSeries {
            label: "Replies".into(),
            values: metrics.iter().map(|m| m.replies as f64).collect(),
            class: "analytics__line--replies".into(),
        },
    ]
}

/// Build the `points` attribute for a polyline from a value series.
fn polyline_points(values: &[f64], min: f64, max: f64, width: f64, height: f64) -> String {
    let n = values.len();
    values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            format!(
                "{:.2},{:.2}",
                line_x(i, n, width, CHART_PADDING),
                line_y(v, min, max, height, CHART_PADDING)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ──────────────────── snapshot resampling / slicing ────────────────

/// Resample daily [`MetricSnapshot::followers_count`] onto a monthly grid.
/// For each `"YYYY-MM"` label, take the last snapshot whose
/// `snapshot_date[0..7]` is `<=` the label (carry-forward); months before
/// the first snapshot map to `0.0`. The output length equals
/// `month_labels.len()` so it index-aligns with [`LineChart`]'s other
/// series. `snapshots` is assumed ascending by `snapshot_date` (as
/// [`crate::analytics::list_metric_snapshots`] returns them).
///
/// Pure — no IO — so it's directly unit-testable.
pub fn snapshot_series_for_months(
    snapshots: &[MetricSnapshot],
    month_labels: &[String],
) -> Vec<f64> {
    month_labels
        .iter()
        .map(|label| {
            let mut val = 0.0;
            for s in snapshots {
                if s.snapshot_date.len() < 7 {
                    continue;
                }
                let month = &s.snapshot_date[0..7];
                if month <= label.as_str() {
                    // Carry-forward: keep the latest in/before this month.
                    val = s.followers_count as f64;
                } else {
                    // Ascending input → no later snapshot can be <= label.
                    break;
                }
            }
            val
        })
        .collect()
}

/// Clamped, exclusive-end slice of a parallel `(labels, values)` series.
/// Out-of-range or inverted ranges yield empty vecs; the two returned vecs
/// are always equal length. Zoom-ready; v1 renders the full range.
///
/// Pure — no IO — so it's directly unit-testable.
pub fn slice_series(
    labels: &[String],
    values: &[f64],
    start_idx: usize,
    end_idx: usize,
) -> (Vec<String>, Vec<f64>) {
    let n = labels.len().min(values.len());
    let start = start_idx.min(n);
    let end = end_idx.min(n);
    if start >= end {
        return (Vec::new(), Vec::new());
    }
    (labels[start..end].to_vec(), values[start..end].to_vec())
}

// ─────────────────────── number formatting ─────────────────────────

/// Compact follower/engagement counts: `1234 → "1.2k"`, `2_500_000 →
/// "2.5M"`. Keeps the ranked-list rows narrow.
fn fmt_count(n: i64) -> String {
    let a = n.unsigned_abs();
    if a >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if a >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Signed compact delta for the summary header (`+1.2k`, `-340`, `0`).
/// `fmt_count` already carries the minus sign for negatives; we only
/// prepend `+` for positives so the badge always reads as a signed change.
fn fmt_delta(n: i64) -> String {
    if n > 0 {
        format!("+{}", fmt_count(n))
    } else {
        fmt_count(n)
    }
}

// ─────────────────────────── view root ─────────────────────────────

#[component]
pub fn AnalyticsView(data: AnalyticsData, #[props(default)] expanded: bool) -> Element {
    let growth = vec![
        ChartSeries {
            label: "Followers".into(),
            values: data.followers_over_time.clone(),
            class: "analytics__line--followers".into(),
        },
        ChartSeries {
            label: "Following".into(),
            values: data.following_over_time.clone(),
            class: "analytics__line--following".into(),
        },
    ];

    // Per-card loading: a card is "loading" (vs done-and-empty) when the
    // backfill phase that fills it hasn't run yet. Ranks mirror the
    // store's BackfillPhase::rank.
    let complete = data.backfill_complete;
    let growth_loading = !complete && data.backfill_phase_rank < PHASE_FOLLOWING;
    // Following line exists but followers (incoming, Constellation) is
    // still being crawled — show a sub-note so the missing blue line
    // doesn't look like a bug.
    let followers_pending = !complete
        && data.backfill_phase_rank < PHASE_FOLLOWERS
        && data.followers_over_time.iter().all(|&v| v == 0.0);
    let followers_loading = !complete && data.top_fans.is_empty() && data.top_mutuals.is_empty();
    let posts_ranking_loading = !complete && data.backfill_phase_rank < PHASE_ENGAGEMENT;

    let growth_caption = if followers_pending {
        "Followers are still backfilling from public follow records (~94% coverage); the blue line fills in shortly. Net counts accrue forward daily."
    } else {
        "Followers reconstructed from public follow records (~94% coverage); exact net counts accrue forward daily."
    };
    let (followers_n, posts_n) = if expanded { (25, 25) } else { (10, 5) };

    rsx! {
        div { class: if expanded { "analytics analytics--expanded" } else { "analytics" },
            // ── Growth ──
            section { class: "analytics__section",
                h3 { class: "analytics__title", "Growth" }
                if growth_loading {
                    div { class: "analytics__loading",
                        span { class: "analytics__spinner" }
                        "Reconstructing your follow history…"
                    }
                } else {
                    LineChart { series: growth, width: LINE_W, height: LINE_H, show_area: true, point_labels: data.growth_labels.clone() }
                    div { class: "analytics__legend",
                        span { class: "analytics__legend-item",
                            span { class: "analytics__swatch analytics__swatch--followers" }
                            "Followers"
                        }
                        span { class: "analytics__legend-item",
                            span { class: "analytics__swatch analytics__swatch--following" }
                            "Following"
                        }
                    }
                    p { class: "analytics__note", "{growth_caption}" }
                }
            }

            // ── Posting volume ──
            section { class: "analytics__section",
                h3 { class: "analytics__title", "Posts per day" }
                BarChart { bars: data.posts_per_day.clone(), max_value: None }
            }

            // ── Posting cadence ──
            section { class: "analytics__section",
                h3 { class: "analytics__title", "Posting cadence" }
                CadenceHeatmap { cells: data.cadence.clone() }
            }

            // ── Top fans ── (engaged followers, composite-ranked)
            section { class: "analytics__section",
                h3 { class: "analytics__title", "Top fans" }
                TopFollowersList { followers: data.top_fans.clone(), loading: followers_loading, limit: followers_n }
            }

            // ── Top mutuals ── (mutual followers ranked by their reach)
            section { class: "analytics__section",
                h3 { class: "analytics__title", "Top mutuals" }
                TopFollowersList { followers: data.top_mutuals.clone(), loading: followers_loading, limit: followers_n }
            }

            // ── Top posts ──
            section { class: "analytics__section",
                h3 { class: "analytics__title", "Top posts" }
                TopPostsList { posts: data.top_posts.clone(), loading: posts_ranking_loading, limit: posts_n }
            }
        }
    }
}

/// Full-screen "pop-out" of the analytics dashboard — a richer, wider
/// layout of the same data. Opened from the Analytics column header.
/// Loads its own [`AnalyticsData`] off the render thread so it's
/// independent of the column's poll cycle.
#[component]
pub fn AnalyticsModal() -> Element {
    let mut expanded = use_context::<Signal<crate::state::AnalyticsExpanded>>();
    // Reactive: read `expanded` synchronously in the closure so opening
    // the pop-out fires the fetch, while a closed modal resolves cheaply
    // to `None` without touching the DB. Hooks run unconditionally per
    // Dioxus rules (mirrors `ThreadSheet`), so the closed-state early
    // return comes *after* `use_resource` — never before it.
    let data = use_resource(move || {
        let is_open = expanded.read().0;
        async move {
            if !is_open {
                return None;
            }
            tokio::task::spawn_blocking(crate::analytics::build_expanded_analytics_data)
                .await
                .ok()
                .and_then(|r| r.ok())
        }
    });
    if !expanded.read().0 {
        return rsx! { Fragment {} };
    }
    let close = move |_| expanded.set(crate::state::AnalyticsExpanded(false));
    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div {
                class: "modal__sheet analytics__modal",
                onclick: move |e| e.stop_propagation(),
                button { class: "profile__close", title: "Close (Esc)", onclick: close,
                    crate::icons::X { size: crate::icons::Size::Sm }
                }
                h2 { class: "analytics__modal-title", "Analytics" }
                match &*data.read_unchecked() {
                    Some(Some(d)) => rsx! { ExpandedAnalyticsView { data: d.clone() } },
                    Some(None) => rsx! { div { class: "analytics__empty", "Couldn't load analytics." } },
                    None => rsx! {
                        div { class: "analytics__loading",
                            span { class: "analytics__spinner" }
                            "Loading…"
                        }
                    },
                }
            }
        }
    }
}

// ───────────────────── expanded (pop-out) view ─────────────────────

/// Which engagement metric the [`TopPostsExpanded`] toggle is ranking by.
/// Each variant maps to a pre-ranked vec the store handed down, so the
/// toggle just swaps which vec renders — no client-side re-sort.
#[derive(Clone, Copy, PartialEq)]
enum MetricSortBy {
    Likes,
    Reposts,
    Replies,
}

/// Which time window the [`TopPostsExpanded`] segmented control is showing.
/// `AllTime` keeps the per-metric (likes/reposts/replies) toggle live; the
/// two bounded windows are like-ranked only and hide the metric toggle.
#[derive(Clone, Copy, PartialEq)]
enum TimeRange {
    AllTime,
    LastYear,
    LastMonth,
}

/// Full deep-dive layout rendered inside the pop-out modal. A superset of
/// [`AnalyticsView`]: reuses the same `LineChart` / `BarChart` /
/// `CadenceHeatmap` / `TopFollowersList` primitives but adds the summary
/// header, the dashed net-followers overlay on the growth chart, the
/// engagement-over-time chart, true per-metric top posts, and four
/// follower lenses. Backed by [`crate::analytics::build_expanded_analytics_data`].
#[component]
pub fn ExpandedAnalyticsView(data: ExpandedAnalyticsData) -> Element {
    let complete = data.backfill_complete;
    let rank = data.backfill_phase_rank;
    let growth_loading = !complete && rank < PHASE_FOLLOWING;
    let engagement_loading = !complete && rank < PHASE_ENGAGEMENT;
    let posts_loading = !complete && rank < PHASE_ENGAGEMENT;
    let followers_loading = !complete && rank < PHASE_FOLLOWERS;

    // Net-followers overlay only draws meaningfully once at least two
    // daily snapshots exist (one point is a flat dot, not a trend). When
    // gated off, the Net series is dropped from the growth vec entirely so
    // LineChart never sees it — and the legend / caption adapt to match.
    // Hold the "net followers" overlay until ~a week of daily snapshots
    // exists — a 2-point line over a multi-year x-axis just reads as a
    // flat, confusing artifact (and "Net 0" for every pre-snapshot month).
    let has_net = data.snapshot_count >= 7;

    // Growth: reconstructed followers + following, plus the snapshot-sourced
    // net-followers overlay as a third, dashed/gray series (CSS-only — no
    // LineChart change), pushed only when the gate is open. The overlay is
    // index-aligned to growth_labels.
    let mut growth = vec![
        ChartSeries {
            label: "Followers".into(),
            values: data.followers_over_time.clone(),
            class: "analytics__line--followers".into(),
        },
        ChartSeries {
            label: "Following".into(),
            values: data.following_over_time.clone(),
            class: "analytics__line--following".into(),
        },
    ];
    if has_net {
        growth.push(ChartSeries {
            label: "Net".into(),
            values: data.net_followers_by_month.clone(),
            class: "analytics__line--net".into(),
        });
    }
    let engagement = engagement_series(&data.engagement_monthly);
    // Month labels for the engagement chart's x-axis + hover tooltip.
    let engagement_labels: Vec<String> = data
        .engagement_monthly
        .iter()
        .map(|m| m.month.clone())
        .collect();

    let growth_caption = if has_net {
        "Solid lines are reconstructed from public follow records (~94% coverage). The dashed gray line is exact net followers from daily snapshots."
    } else {
        "Solid lines reconstructed from public follow records (~94% coverage). Daily snapshots begin accruing for exact net-follower tracking."
    };

    rsx! {
        div { class: "analytics analytics__deepdive",
            SummaryStatsHeader { stats: data.summary.clone() }

            // ── Two-column body: charts (main) + follower lenses (side) ──
            div { class: "analytics__body",
                div { class: "analytics__col-main",
                    // ── Growth (followers / following / optional net overlay) ──
                    section { class: "analytics__section",
                        h3 { class: "analytics__title", "Growth" }
                        if growth_loading {
                            div { class: "analytics__loading",
                                span { class: "analytics__spinner" }
                                "Reconstructing your follow history…"
                            }
                        } else {
                            LineChart { series: growth, width: LINE_W, height: LINE_H, show_area: true, point_labels: data.growth_labels.clone() }
                            div { class: "analytics__legend",
                                span { class: "analytics__legend-item",
                                    span { class: "analytics__swatch analytics__swatch--followers" }
                                    "Followers"
                                }
                                span { class: "analytics__legend-item",
                                    span { class: "analytics__swatch analytics__swatch--following" }
                                    "Following"
                                }
                                if has_net {
                                    span { class: "analytics__legend-item",
                                        span { class: "analytics__swatch analytics__swatch--net" }
                                        "Net followers (measured daily)"
                                    }
                                }
                            }
                            p { class: "analytics__note", "{growth_caption}" }
                        }
                    }

                    // ── Posting volume ──
                    section { class: "analytics__section",
                        h3 { class: "analytics__title", "Posts per day" }
                        BarChart { bars: data.posts_per_day.clone(), max_value: None }
                    }

                    // ── Posting cadence ──
                    section { class: "analytics__section",
                        h3 { class: "analytics__title", "Posting cadence" }
                        CadenceHeatmap { cells: data.cadence.clone() }
                    }

                    // ── Engagement over time ──
                    section { class: "analytics__section",
                        h3 { class: "analytics__title", "Engagement over time" }
                        if engagement_loading {
                            div { class: "analytics__loading",
                                span { class: "analytics__spinner" }
                                "Charting engagement once the backfill finishes…"
                            }
                        } else {
                            LineChart { series: engagement, width: LINE_W, height: LINE_H, show_area: false, point_labels: engagement_labels }
                            div { class: "analytics__legend",
                                span { class: "analytics__legend-item",
                                    span { class: "analytics__swatch analytics__swatch--likes" }
                                    "Likes"
                                }
                                span { class: "analytics__legend-item",
                                    span { class: "analytics__swatch analytics__swatch--reposts" }
                                    "Reposts"
                                }
                                span { class: "analytics__legend-item",
                                    span { class: "analytics__swatch analytics__swatch--replies" }
                                    "Replies"
                                }
                            }
                        }
                    }
                    // ── Top posts: lives in the charts column (chart-width)
                    // so it fills the left-column whitespace next to the
                    // tall follower-lens stack, rather than full-width below. ──
                    section { class: "analytics__section",
                        h3 { class: "analytics__title", "Top posts" }
                        TopPostsExpanded {
                            all_time: data.top_posts_all_time.clone(),
                            last_year: data.top_posts_last_year.clone(),
                            last_month: data.top_posts_last_month.clone(),
                            by_likes: data.top_posts_by_likes.clone(),
                            by_reposts: data.top_posts_by_reposts.clone(),
                            by_replies: data.top_posts_by_replies.clone(),
                            loading: posts_loading,
                            limit: 25,
                        }
                    }
                }

                // ── Follower lenses ── Ordered most-useful-first:
                // your engaged fans, influential mutuals, high-reach
                // accounts you could follow back, then silent reach.
                div { class: "analytics__col-side",
                    FollowerLensCard {
                        title: "Top Fans",
                        subtitle: "Engage with you most (replies, mentions, quotes)",
                        followers: data.top_fans.clone(),
                        loading: followers_loading,
                        limit: 50,
                    }
                    FollowerLensCard {
                        title: "Mutuals by Reach",
                        subtitle: "Your most influential mutual followers",
                        followers: data.mutuals_by_reach.clone(),
                        loading: followers_loading,
                        limit: 50,
                    }
                    FollowerLensCard {
                        title: "High Clout (Not Mutual)",
                        subtitle: "High reach, you don't follow back",
                        followers: data.high_clout_not_mutual.clone(),
                        loading: followers_loading,
                        limit: 50,
                    }
                    FollowerLensCard {
                        title: "Lurkers with Clout",
                        subtitle: "Silent high-reach followers",
                        followers: data.lurkers_by_clout.clone(),
                        loading: followers_loading,
                        limit: 50,
                    }
                }
            }
        }
    }
}

/// Full-width summary header: current followers / following / posts with
/// 7-day and 30-day delta badges (computed store-side by
/// [`crate::analytics::metric_deltas`]). Negative deltas flip the badge to
/// the warning style.
#[component]
fn SummaryStatsHeader(stats: SummaryStats) -> Element {
    let cards: [(&str, i64, i64, i64); 3] = [
        (
            "Followers",
            stats.current_followers,
            stats.delta_followers_7d,
            stats.delta_followers_30d,
        ),
        (
            "Following",
            stats.current_following,
            stats.delta_following_7d,
            stats.delta_following_30d,
        ),
        (
            "Posts",
            stats.current_posts,
            stats.delta_posts_7d,
            stats.delta_posts_30d,
        ),
    ];
    rsx! {
        section { class: "analytics__section analytics__header-grid",
            for (label , value , d7 , d30) in cards {
                div { key: "{label}", class: "analytics__stat-card",
                    span { class: "analytics__stat-value", "{fmt_count(value)}" }
                    span { class: "analytics__stat-label", "{label}" }
                    div { class: "analytics__stat-deltas",
                        span {
                            class: if d7 < 0 { "analytics__delta-badge analytics__delta-badge--negative" } else { "analytics__delta-badge" },
                            "{fmt_delta(d7)} (7d)"
                        }
                        span {
                            class: if d30 < 0 { "analytics__delta-badge analytics__delta-badge--negative" } else { "analytics__delta-badge" },
                            "{fmt_delta(d30)} (30d)"
                        }
                    }
                }
            }
        }
    }
}

/// Top posts with a time-range segmented control (All time / Last year /
/// Last month) and — in All-time mode only — a likes/reposts/replies
/// metric toggle. The bounded windows are like-ranked store cuts; the
/// All-time view swaps between the three pre-ranked metric vecs (no
/// client-side re-sort). Each row is click-through: it focuses the post's
/// thread in the main column and closes the pop-out.
#[component]
fn TopPostsExpanded(
    all_time: Vec<PostMetric>,
    last_year: Vec<PostMetric>,
    last_month: Vec<PostMetric>,
    by_likes: Vec<PostMetric>,
    by_reposts: Vec<PostMetric>,
    by_replies: Vec<PostMetric>,
    loading: bool,
    limit: usize,
) -> Element {
    let mut time_range = use_signal(|| TimeRange::AllTime);
    let mut sort_by = use_signal(|| MetricSortBy::Likes);
    let mut thread_focus = use_context::<Signal<crate::state::ThreadFocus>>();
    let mut expanded = use_context::<Signal<crate::state::AnalyticsExpanded>>();

    if loading {
        // Until the engagement backfill runs, every count is 0 and a
        // "top" ranking would just be most-recent — misleading.
        return rsx! {
            div { class: "analytics__loading",
                span { class: "analytics__spinner" }
                "Ranking by engagement once the backfill finishes…"
            }
        };
    }

    let range = time_range();
    let show_metric_toggle = range == TimeRange::AllTime;
    // All-time honors the metric toggle (swapping the pre-ranked vec);
    // the bounded windows are like-ranked store cuts. `all_time` ==
    // `by_likes`, so All-time/Likes reuses the metric vec directly.
    let rows = match range {
        TimeRange::AllTime => match sort_by() {
            MetricSortBy::Likes => &by_likes,
            MetricSortBy::Reposts => &by_reposts,
            MetricSortBy::Replies => &by_replies,
        },
        TimeRange::LastYear => &last_year,
        TimeRange::LastMonth => &last_month,
    };
    // Silence the unused-binding lint for the all-time-only alias while
    // keeping the explicit prop in the signature.
    let _ = &all_time;
    let shown = rows.len().min(limit);

    rsx! {
        div { class: "analytics__time-range",
            for (variant , label) in [
                (TimeRange::AllTime, "All time"),
                (TimeRange::LastYear, "Last year"),
                (TimeRange::LastMonth, "Last month"),
            ] {
                button {
                    key: "{label}",
                    class: if range == variant { "analytics__time-range-btn analytics__time-range-btn--active" } else { "analytics__time-range-btn" },
                    onclick: move |_| time_range.set(variant),
                    "{label}"
                }
            }
        }
        if show_metric_toggle {
            div { class: "analytics__metric-toggle",
                for (variant , label) in [
                    (MetricSortBy::Likes, "Likes"),
                    (MetricSortBy::Reposts, "Reposts"),
                    (MetricSortBy::Replies, "Replies"),
                ] {
                    button {
                        key: "{label}",
                        class: if sort_by() == variant { "analytics__metric-toggle-btn analytics__metric-toggle-btn--active" } else { "analytics__metric-toggle-btn" },
                        onclick: move |_| sort_by.set(variant),
                        "{label}"
                    }
                }
            }
        }
        if rows.is_empty() {
            div { class: "analytics__empty", "No posts captured yet…" }
        } else {
            ol { class: "analytics__posts-list",
                for p in rows.iter().take(shown) {
                    {
                        let uri = p.uri.clone();
                        let rkey = p.rkey.clone();
                        let text = p.text_preview.clone().unwrap_or_default();
                        let like = p.like_count;
                        let repost = p.repost_count;
                        let reply = p.reply_count;
                        let date = p.ts.format("%b %d").to_string();
                        rsx! {
                            li {
                                key: "{rkey}",
                                class: "analytics__posts-row analytics__posts-row--clickable",
                                onclick: move |_| {
                                    thread_focus.set(crate::state::ThreadFocus(Some(uri.clone())));
                                    expanded.set(crate::state::AnalyticsExpanded(false));
                                },
                                p { class: "analytics__post-text", "{text}" }
                                div { class: "analytics__post-stats",
                                    span { class: "analytics__stat", title: "Likes", "♥ {fmt_count(like)}" }
                                    span { class: "analytics__stat", title: "Reposts", "⇄ {fmt_count(repost)}" }
                                    span { class: "analytics__stat", title: "Replies", "💬 {fmt_count(reply)}" }
                                    span { class: "analytics__post-date", "{date}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One titled follower-lens card wrapping the existing [`TopFollowersList`]
/// (which owns the loading / empty states). Used four times in the pop-out
/// for the top-fans / high-clout / mutuals / lurkers cuts.
#[component]
fn FollowerLensCard(
    title: String,
    subtitle: String,
    followers: Vec<FollowerStat>,
    loading: bool,
    limit: usize,
) -> Element {
    rsx! {
        section { class: "analytics__lens-card",
            h3 { class: "analytics__title analytics__lens-title", "{title}" }
            p { class: "analytics__lens-subtitle", "{subtitle}" }
            TopFollowersList { followers, loading, limit }
        }
    }
}

// ─────────────────────────── line chart ────────────────────────────

/// Interactive multi-series line chart. Reusable for Growth (followers /
/// following / optional net overlay) and Engagement (likes / reposts /
/// replies). Beyond the polylines it renders:
///
/// - a y-axis scale (3 evenly-spaced tick labels via [`y_axis_ticks`]),
/// - x-axis labels subsampled from `point_labels` via [`x_axis_label_indices`],
/// - full-height transparent **hit rects** (one per data slot) that drive
///   a `hover_index` signal, so hovering anywhere in a column registers,
/// - on hover: a vertical **guide line**, **enlarged markers** at that
///   index for every series, and an HTML **tooltip** (label + per-series
///   value) positioned over the hovered column.
///
/// The overlay/net series is the caller's concern — `LineChart` is
/// overlay-agnostic and just renders whatever series it's handed.
#[component]
fn LineChart(
    series: Vec<ChartSeries>,
    width: f64,
    height: f64,
    show_area: bool,
    /// Optional x-axis labels (one per data point) — drives both the
    /// x-axis tick labels and the hover tooltip's header. Empty = no
    /// x-axis labels / a value-only tooltip.
    #[props(default)]
    point_labels: Vec<String>,
) -> Element {
    let mut hover_index = use_signal::<Option<usize>>(|| None);

    // Shared y-scale across every series so the curves are directly
    // comparable. Floor the range at the data extent; if there's no
    // data (or one flat value) the helpers fall back to the baseline.
    let all: Vec<f64> = series
        .iter()
        .flat_map(|s| s.values.iter().copied())
        .collect();
    let has_data = series.iter().any(|s| s.values.len() >= 2);
    if !has_data {
        return rsx! {
            div { class: "analytics__empty", "Collecting data…" }
        };
    }
    let max = all.iter().copied().fold(f64::MIN, f64::max);
    let min = all.iter().copied().fold(f64::MAX, f64::min);

    // One hover "slot" per data point across the widest series.
    let n = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    let slot_w = if n > 0 {
        (width - 2.0 * CHART_PADDING) / n as f64
    } else {
        width
    };

    // Three horizontal gridlines (top / mid / baseline) for reference.
    let grid_ys: Vec<f64> = (0..=2)
        .map(|i| CHART_PADDING + i as f64 * (height - 2.0 * CHART_PADDING) / 2.0)
        .collect();
    let base_y = height - CHART_PADDING;

    let y_ticks = y_axis_ticks(min, max, 3);
    let x_label_idxs = x_axis_label_indices(point_labels.len(), 6);
    let hovered = hover_index();

    rsx! {
        div { class: "analytics__chart-wrap", onmouseleave: move |_| hover_index.set(None),
            svg {
                class: "analytics__chart",
                width: "100%",
                view_box: "0 0 {width} {height}",
                role: "img",
                // Reference gridlines.
                for (gi , gy) in grid_ys.iter().enumerate() {
                    line {
                        key: "grid-{gi}",
                        class: "analytics__grid-line",
                        x1: "{CHART_PADDING}",
                        y1: "{gy}",
                        x2: "{width - CHART_PADDING}",
                        y2: "{gy}",
                    }
                }
                // Y-axis scale labels (value at each tick).
                for (ti , tv) in y_ticks.iter().enumerate() {
                    text {
                        key: "yt-{ti}",
                        class: "analytics__axis-label",
                        x: "2",
                        y: "{line_y(*tv, min, max, height, CHART_PADDING):.2}",
                        text_anchor: "start",
                        dominant_baseline: "middle",
                        "{*tv as i64}"
                    }
                }
                // X-axis labels, subsampled to ~6 evenly-spaced points.
                for idx in x_label_idxs.iter() {
                    if let Some(label) = point_labels.get(*idx) {
                        text {
                            key: "xt-{idx}",
                            class: "analytics__axis-label",
                            x: "{line_x(*idx, n, width, CHART_PADDING):.2}",
                            y: "{height - 2.0}",
                            text_anchor: "middle",
                            dominant_baseline: "hanging",
                            "{label}"
                        }
                    }
                }
                // Series areas + polylines.
                for s in series.iter() {
                    if s.values.len() >= 2 {
                        {
                            let pts = polyline_points(&s.values, min, max, width, height);
                            let first_x = line_x(0, s.values.len(), width, CHART_PADDING);
                            let last_x = line_x(s.values.len() - 1, s.values.len(), width, CHART_PADDING);
                            let area_pts = format!("{pts} {last_x:.2},{base_y:.2} {first_x:.2},{base_y:.2}");
                            rsx! {
                                if show_area {
                                    polygon {
                                        key: "area-{s.label}",
                                        class: "analytics__area {s.class}",
                                        points: "{area_pts}",
                                    }
                                }
                                polyline {
                                    key: "line-{s.label}",
                                    class: "analytics__line {s.class}",
                                    points: "{pts}",
                                    fill: "none",
                                }
                            }
                        }
                    }
                }
                // Hover overlay: vertical guide + enlarged per-series markers.
                if let Some(idx) = hovered {
                    {
                        let gx = line_x(idx, n, width, CHART_PADDING);
                        rsx! {
                            line {
                                class: "analytics__chart-guide",
                                x1: "{gx:.2}",
                                y1: "{CHART_PADDING}",
                                x2: "{gx:.2}",
                                y2: "{height - CHART_PADDING}",
                            }
                            for s in series.iter() {
                                if idx < s.values.len() {
                                    circle {
                                        key: "hov-{s.label}",
                                        class: "analytics__point {s.class} analytics__point--hovered",
                                        cx: "{gx:.2}",
                                        cy: "{line_y(s.values[idx], min, max, height, CHART_PADDING):.2}",
                                        r: "4.5",
                                    }
                                }
                            }
                        }
                    }
                }
                // Full-height transparent hit rects per slot — hovering
                // anywhere in a column selects that index.
                for i in 0..n {
                    rect {
                        key: "hit-{i}",
                        class: "analytics__chart-hit",
                        x: "{line_x(i, n, width, CHART_PADDING) - slot_w / 2.0:.2}",
                        y: "0",
                        width: "{slot_w:.2}",
                        height: "{height}",
                        fill: "transparent",
                        onmouseenter: move |_| hover_index.set(Some(i)),
                        onmousemove: move |_| hover_index.set(Some(i)),
                    }
                }
            }
            // HTML tooltip, positioned over the hovered column (CSS
            // translateX(-50%) centers it on the % left).
            if let Some(idx) = hovered {
                div {
                    class: "analytics__chart-tooltip",
                    style: "left: {line_x(idx, n, width, CHART_PADDING) / width * 100.0:.2}%;",
                    if let Some(l) = point_labels.get(idx) {
                        div { class: "analytics__tooltip-label", "{l}" }
                    }
                    for s in series.iter() {
                        if let Some(v) = s.values.get(idx) {
                            div { class: "analytics__tooltip-row",
                                span { class: "analytics__tooltip-name", "{s.label}" }
                                span { class: "analytics__tooltip-value", "{*v as i64}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ──────────────────────────── bar chart ────────────────────────────

#[component]
fn BarChart(bars: Vec<BarDatum>, max_value: Option<f64>) -> Element {
    if bars.is_empty() {
        return rsx! {
            div { class: "analytics__empty", "Collecting data…" }
        };
    }
    let width = LINE_W;
    let height = 120.0;
    let chart_h = height - 2.0 * CHART_PADDING;
    let n = bars.len();
    let max = max_value
        .unwrap_or_else(|| bars.iter().map(|b| b.value).fold(0.0, f64::max))
        .max(1.0);
    let slot_w = (width - 2.0 * CHART_PADDING) / n as f64;
    // Leave a small gap between bars; never let the drawn width go to 0.
    let bar_w = (slot_w * 0.7).max(1.0);

    rsx! {
        svg {
            class: "analytics__chart",
            width: "100%",
            view_box: "0 0 {width} {height}",
            preserve_aspect_ratio: "none",
            role: "img",
            line {
                class: "analytics__grid-line",
                x1: "{CHART_PADDING}",
                y1: "{height - CHART_PADDING}",
                x2: "{width - CHART_PADDING}",
                y2: "{height - CHART_PADDING}",
            }
            for (i , b) in bars.iter().enumerate() {
                {
                    let bh = bar_height(b.value, max, chart_h);
                    let x = CHART_PADDING + i as f64 * slot_w + (slot_w - bar_w) / 2.0;
                    let y = height - CHART_PADDING - bh;
                    rsx! {
                        rect {
                            key: "bar-{i}",
                            class: "analytics__bar",
                            x: "{x:.2}",
                            y: "{y:.2}",
                            width: "{bar_w:.2}",
                            height: "{bh:.2}",
                            rx: "1",
                            "data-label": "{b.label}",
                            // Native SVG tooltip on hover.
                            title { "{b.label}: {b.value as i64}" }
                        }
                    }
                }
            }
        }
    }
}

// ─────────────────────────── heatmap ───────────────────────────────

/// Weekday row labels for the cadence grid (row 0 == Monday, matching
/// [`crate::analytics::cadence_cells`]'s `num_days_from_monday`).
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

#[component]
fn CadenceHeatmap(cells: Vec<Vec<f64>>) -> Element {
    // Treat a missing/short grid as "no data yet".
    let has_data = cells.iter().any(|row| row.iter().any(|&v| v > 0.0));
    if !has_data {
        return rsx! {
            div { class: "analytics__empty", "Collecting data…" }
        };
    }
    let cell = 11.0_f64;
    let gutter = 30.0_f64; // left room for weekday labels
    let top = 14.0_f64; // top room for hour ticks
    let cols = 24.0;
    let rows = 7.0;
    let width = gutter + cols * cell;
    let height = top + rows * cell;

    rsx! {
        svg {
            class: "analytics__heatmap",
            width: "100%",
            view_box: "0 0 {width} {height}",
            role: "img",
            // Hour ticks every 6h.
            for h in [0usize, 6, 12, 18] {
                text {
                    key: "hour-{h}",
                    class: "analytics__axis",
                    x: "{gutter + h as f64 * cell}",
                    y: "{top - 4.0}",
                    "{h}"
                }
            }
            for (w , label) in WEEKDAYS.iter().enumerate() {
                text {
                    key: "wd-{w}",
                    class: "analytics__axis",
                    x: "0",
                    y: "{top + w as f64 * cell + cell * 0.8}",
                    "{label}"
                }
            }
            for (w , row) in cells.iter().enumerate().take(7) {
                for (h , value) in row.iter().enumerate().take(24) {
                    {
                        // Floor the opacity so empty cells still read as a
                        // faint grid rather than vanishing into the bg.
                        let opacity = 0.06 + value * 0.94;
                        rsx! {
                            rect {
                                key: "c-{w}-{h}",
                                class: "analytics__heatmap-cell",
                                x: "{gutter + h as f64 * cell}",
                                y: "{top + w as f64 * cell}",
                                width: "{cell - 1.0}",
                                height: "{cell - 1.0}",
                                rx: "1",
                                fill_opacity: "{opacity:.3}",
                                title { "{WEEKDAYS[w]} {h}:00 · {(value * 100.0) as i64}% of peak" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ───────────────────────── ranked lists ────────────────────────────

#[component]
fn TopFollowersList(followers: Vec<FollowerStat>, loading: bool, limit: usize) -> Element {
    if followers.is_empty() {
        // Distinguish "still computing" from "genuinely none" so an
        // empty mid-backfill card doesn't read as a finished result.
        return rsx! {
            if loading {
                div { class: "analytics__loading",
                    span { class: "analytics__spinner" }
                    "Analyzing your followers…"
                }
            } else {
                div { class: "analytics__empty", "No follower data." }
            }
        };
    }
    let shown = followers.len().min(limit);
    rsx! {
        ol { class: "analytics__followers-list",
            for (i , f) in followers.iter().take(shown).enumerate() {
                li {
                    key: "{f.follower_did}",
                    class: "analytics__followers-row",
                    span { class: "analytics__rank-badge", "{i + 1}" }
                    if let Some(avatar) = &f.follower_avatar {
                        img {
                            class: "analytics__avatar",
                            src: "{avatar}",
                            alt: "{f.follower_handle}",
                            loading: "lazy",
                            decoding: "async",
                        }
                    } else {
                        span { class: "analytics__avatar analytics__avatar--placeholder" }
                    }
                    div { class: "analytics__follower-id",
                        span { class: "analytics__follower-name",
                            "{f.follower_display_name.clone().unwrap_or_else(|| f.follower_handle.clone())}"
                        }
                        span { class: "analytics__follower-handle", "@{f.follower_handle}" }
                    }
                    div { class: "analytics__follower-stats",
                        span { class: "analytics__stat", title: "Reach (their followers)",
                            "{fmt_count(f.followers_count)}"
                        }
                        if f.mutual {
                            span { class: "analytics__pill analytics__pill--mutual", "mutual" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TopPostsList(posts: Vec<PostMetric>, loading: bool, limit: usize) -> Element {
    if posts.is_empty() || loading {
        // `loading` is set until the engagement backfill has run — until
        // then like/repost counts are all 0 and a "top" ranking would
        // just be most-recent, which is misleading.
        return rsx! {
            if loading {
                div { class: "analytics__loading",
                    span { class: "analytics__spinner" }
                    "Ranking by engagement once the backfill finishes…"
                }
            } else {
                div { class: "analytics__empty", "No posts captured yet…" }
            }
        };
    }
    let shown = posts.len().min(limit);
    rsx! {
        ol { class: "analytics__posts-list",
            for p in posts.iter().take(shown) {
                li {
                    key: "{p.rkey}",
                    class: "analytics__posts-row",
                    p { class: "analytics__post-text",
                        "{p.text_preview.clone().unwrap_or_default()}"
                    }
                    div { class: "analytics__post-stats",
                        span { class: "analytics__stat", title: "Likes", "♥ {fmt_count(p.like_count)}" }
                        span { class: "analytics__stat", title: "Reposts", "⇄ {fmt_count(p.repost_count)}" }
                        span { class: "analytics__stat", title: "Replies", "💬 {fmt_count(p.reply_count)}" }
                        span { class: "analytics__post-date", "{p.ts.format(\"%b %d\")}" }
                    }
                }
            }
        }
    }
}

// ──────────────────────────── tests ────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const W: f64 = 320.0;
    const H: f64 = 140.0;
    const P: f64 = CHART_PADDING;

    #[test]
    fn line_x_spans_padding_to_width_minus_padding() {
        let n = 10;
        assert!(
            (line_x(0, n, W, P) - P).abs() < 1e-9,
            "first x should be padding"
        );
        assert!(
            (line_x(n - 1, n, W, P) - (W - P)).abs() < 1e-9,
            "last x should be width - padding"
        );
        // Midpoint lands in the middle of the span.
        let mid = line_x(1, 3, W, P);
        assert!((mid - W / 2.0).abs() < 1e-9);
    }

    #[test]
    fn line_x_single_and_empty_no_panic() {
        // n == 1 and n == 0 must not divide by (n - 1).
        assert_eq!(line_x(0, 1, W, P), P);
        assert_eq!(line_x(0, 0, W, P), P);
    }

    #[test]
    fn line_y_maps_extremes_to_top_and_baseline() {
        let (min, max) = (0.0, 100.0);
        assert!(
            (line_y(max, min, max, H, P) - P).abs() < 1e-9,
            "max value should map to top padding"
        );
        assert!(
            (line_y(min, min, max, H, P) - (H - P)).abs() < 1e-9,
            "min value should map to baseline"
        );
        // Halfway value lands halfway up the drawing area.
        let midy = line_y(50.0, min, max, H, P);
        let expect = H - P - 0.5 * (H - 2.0 * P);
        assert!((midy - expect).abs() < 1e-9);
    }

    #[test]
    fn line_y_flat_range_rests_on_baseline() {
        // max == min would divide by zero; must fall back to baseline.
        assert_eq!(line_y(5.0, 5.0, 5.0, H, P), H - P);
    }

    #[test]
    fn bar_height_scales_against_max() {
        let chart_h = 100.0;
        assert_eq!(bar_height(50.0, 100.0, chart_h), 50.0);
        assert_eq!(bar_height(100.0, 100.0, chart_h), 100.0);
        assert_eq!(bar_height(0.0, 100.0, chart_h), 0.0);
    }

    #[test]
    fn bar_height_zero_max_no_divide_by_zero() {
        // Empty data → max 0 → no NaN/inf, just a zero-height bar.
        assert_eq!(bar_height(3.0, 0.0, 100.0), 0.0);
        assert!(bar_height(3.0, 0.0, 100.0).is_finite());
    }

    #[test]
    fn polyline_points_has_one_pair_per_value() {
        let pts = polyline_points(&[0.0, 50.0, 100.0], 0.0, 100.0, W, H);
        assert_eq!(pts.split(' ').count(), 3);
        // First pair is at (padding, baseline) for value == min.
        assert!(pts.starts_with(&format!("{:.2},{:.2}", P, H - P)));
    }

    #[test]
    fn fmt_count_thresholds() {
        assert_eq!(fmt_count(0), "0");
        assert_eq!(fmt_count(999), "999");
        assert_eq!(fmt_count(1_500), "1.5k");
        assert_eq!(fmt_count(2_500_000), "2.5M");
    }

    #[test]
    fn y_axis_ticks_even_spacing() {
        // 3 ticks across [0, 1040] → endpoints + midpoint.
        assert_eq!(y_axis_ticks(0.0, 1040.0, 3), vec![0.0, 520.0, 1040.0]);
        // First/last always land on the extremes.
        let ticks = y_axis_ticks(10.0, 70.0, 4);
        assert_eq!(ticks.first(), Some(&10.0));
        assert_eq!(ticks.last(), Some(&70.0));
        assert_eq!(ticks.len(), 4);
    }

    #[test]
    fn y_axis_ticks_flat_range_guard() {
        // min == max → a single tick, no NaN / divide-by-zero.
        assert_eq!(y_axis_ticks(5.0, 5.0, 3), vec![5.0]);
        // Inverted range also collapses to one tick.
        assert_eq!(y_axis_ticks(9.0, 2.0, 3), vec![9.0]);
        // count < 2 can't span a range → single tick.
        assert_eq!(y_axis_ticks(0.0, 100.0, 1), vec![0.0]);
    }

    #[test]
    fn x_axis_label_indices_subsamples() {
        // 60 labels, ~6 picks: anchored on 0 and the last index, ascending.
        let idxs = x_axis_label_indices(60, 6);
        assert_eq!(idxs.first(), Some(&0));
        assert_eq!(idxs.last(), Some(&59));
        assert!(idxs.len() <= 6 && idxs.len() >= 5);
        // Strictly ascending, unique.
        assert!(idxs.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn x_axis_label_indices_fewer_than_target() {
        // Fewer points than the target → every index, in order.
        assert_eq!(x_axis_label_indices(3, 6), vec![0, 1, 2]);
    }

    #[test]
    fn x_axis_label_indices_empty() {
        assert_eq!(x_axis_label_indices(0, 6), Vec::<usize>::new());
    }

    fn snap(date: &str, followers: i64) -> MetricSnapshot {
        MetricSnapshot {
            snapshot_date: date.into(),
            ts: chrono::DateTime::parse_from_rfc3339(&format!("{date}T00:00:00Z"))
                .unwrap()
                .with_timezone(&chrono::Utc),
            followers_count: followers,
            following_count: 0,
            posts_count: 0,
        }
    }

    #[test]
    fn snapshot_series_for_months_carry_forward_and_alignment() {
        // Snapshots ascending by date, sparse across months.
        let snaps = vec![
            snap("2026-02-15", 100),
            snap("2026-02-20", 120), // later in Feb → Feb resolves to 120
            snap("2026-04-10", 200), // March has no snapshot
        ];
        let labels: Vec<String> = ["2026-01", "2026-02", "2026-03", "2026-04", "2026-05"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let series = snapshot_series_for_months(&snaps, &labels);
        // Output length tracks labels exactly (LineChart alignment).
        assert_eq!(series.len(), labels.len());
        // Jan is before the first snapshot → 0.0.
        assert_eq!(series[0], 0.0);
        // Feb resolves to the LAST in-month snapshot (120, not 100).
        assert_eq!(series[1], 120.0);
        // Mar has no snapshot → carry forward the latest in/before (Feb → 120).
        assert_eq!(series[2], 120.0);
        // Apr resolves to its own snapshot.
        assert_eq!(series[3], 200.0);
        // May (after last snapshot) carries forward Apr's value.
        assert_eq!(series[4], 200.0);
    }

    #[test]
    fn snapshot_series_for_months_empty_inputs() {
        assert!(snapshot_series_for_months(&[], &[]).is_empty());
        let labels: Vec<String> = vec!["2026-01".into(), "2026-02".into()];
        // No snapshots → all zero, still label-aligned.
        assert_eq!(snapshot_series_for_months(&[], &labels), vec![0.0, 0.0]);
    }

    #[test]
    fn slice_series_clamps_and_inverts() {
        let labels: Vec<String> = (0..5).map(|i| format!("l{i}")).collect();
        let values: Vec<f64> = (0..5).map(|i| i as f64).collect();

        // Full passthrough.
        let (l, v) = slice_series(&labels, &values, 0, 5);
        assert_eq!(l.len(), 5);
        assert_eq!(v, values);

        // Tail half.
        let (l, v) = slice_series(&labels, &values, 2, 5);
        assert_eq!(l, vec!["l2", "l3", "l4"]);
        assert_eq!(v, vec![2.0, 3.0, 4.0]);

        // End clamps past the length.
        let (l, v) = slice_series(&labels, &values, 3, 99);
        assert_eq!(l.len(), 2);
        assert_eq!(v, vec![3.0, 4.0]);

        // Inverted range → empty, equal length.
        let (l, v) = slice_series(&labels, &values, 5, 0);
        assert!(l.is_empty() && v.is_empty());

        // Empty inputs → empty.
        let (l, v) = slice_series(&[], &[], 0, 3);
        assert!(l.is_empty() && v.is_empty());
    }
}
