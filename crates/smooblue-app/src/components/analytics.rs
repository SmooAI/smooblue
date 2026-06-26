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

use crate::analytics::{FollowerStat, PostMetric};
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
    /// Cumulative followers per day, oldest → newest.
    pub followers_over_time: Vec<f64>,
    /// Cumulative following per day, oldest → newest.
    pub following_over_time: Vec<f64>,
    /// One bar per calendar day (count of own posts).
    pub posts_per_day: Vec<BarDatum>,
    /// `[7][24]` (weekday × hour) posting cadence, normalized `0.0..=1.0`.
    pub cadence: Vec<Vec<f64>>,
    /// Ranked follower cut (best fans first).
    pub top_followers: Vec<FollowerStat>,
    /// Top own posts by like count.
    pub top_posts: Vec<PostMetric>,
}

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

// ─────────────────────────── view root ─────────────────────────────

#[component]
pub fn AnalyticsView(data: AnalyticsData) -> Element {
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

    rsx! {
        div { class: "analytics",
            // ── Growth ──
            section { class: "analytics__section",
                h3 { class: "analytics__title", "Growth" }
                LineChart { series: growth, width: LINE_W, height: LINE_H, show_area: true }
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
                p { class: "analytics__note",
                    "Followers reconstructed from public follow records (~94% coverage); exact net counts accrue forward daily."
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

            // ── Best followers ──
            section { class: "analytics__section",
                h3 { class: "analytics__title", "Best followers" }
                TopFollowersList { followers: data.top_followers.clone() }
            }

            // ── Top posts ──
            section { class: "analytics__section",
                h3 { class: "analytics__title", "Top posts" }
                TopPostsList { posts: data.top_posts.clone() }
            }
        }
    }
}

// ─────────────────────────── line chart ────────────────────────────

#[component]
fn LineChart(series: Vec<ChartSeries>, width: f64, height: f64, show_area: bool) -> Element {
    // Shared y-scale across every series so the two curves are directly
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

    // Three horizontal gridlines (top / mid / baseline) for reference.
    let grid_ys: Vec<f64> = (0..=2)
        .map(|i| CHART_PADDING + i as f64 * (height - 2.0 * CHART_PADDING) / 2.0)
        .collect();
    let base_y = height - CHART_PADDING;

    rsx! {
        svg {
            class: "analytics__chart",
            width: "100%",
            view_box: "0 0 {width} {height}",
            preserve_aspect_ratio: "none",
            role: "img",
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
fn TopFollowersList(followers: Vec<FollowerStat>) -> Element {
    if followers.is_empty() {
        return rsx! {
            div { class: "analytics__empty", "No follower stats yet — enriching in the background…" }
        };
    }
    rsx! {
        ol { class: "analytics__followers-list",
            for (i , f) in followers.iter().enumerate() {
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
fn TopPostsList(posts: Vec<PostMetric>) -> Element {
    if posts.is_empty() {
        return rsx! {
            div { class: "analytics__empty", "No posts captured yet…" }
        };
    }
    rsx! {
        ol { class: "analytics__posts-list",
            for p in posts.iter() {
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
}
