//! Independently authored Rust scoreboard with public Data Dragon artwork.
use crate::{App, i18n::{Lang, Msg}, model::{Match}, presentation::{Badge, plain}, render_assets::{Assets, Wanted}};
use anyhow::{Result, ensure};
use resvg::usvg::fontdb;
use serde_json::Value;
use std::{collections::HashSet, path::Path, sync::{Arc, OnceLock}};

pub(crate) const DIM: &str = "#9ca8bb";
pub(crate) const GOLD: &str = "#edc976";
pub(crate) const RED: &str = "#fa8297";

pub(crate) struct Canvas { pub svg: String, pub clips: usize }
pub(crate) fn escape(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).map(|c| match c {
        '&' => "&amp;".into(), '<' => "&lt;".into(), '>' => "&gt;".into(), '\'' => "&apos;".into(), '"' => "&quot;".into(), _ => c.to_string(),
    }).collect()
}
pub(crate) fn short(text: &str, count: usize) -> String {
    let text = plain(text);
    if text.chars().count() <= count { text } else { format!("{}…", text.chars().take(count.saturating_sub(1)).collect::<String>()) }
}
pub(crate) fn number(value: i64) -> String {
    let digits = value.to_string();
    digits.chars().rev().enumerate().flat_map(|(i, c)| if i > 0 && i % 3 == 0 { vec![',', c] } else { vec![c] }).collect::<Vec<_>>().into_iter().rev().collect()
}
pub(crate) fn team_name(team: u16, lang: Lang) -> String { lang.team(team).to_uppercase() }

pub(crate) fn tier_color(tier: &str) -> &'static str {
    match tier {
        "IRON" => "#9a9a9a", "BRONZE" => "#c58b65", "SILVER" => "#b8c4d0", "GOLD" => "#e6c06a",
        "PLATINUM" => "#4fc1b8", "EMERALD" => "#3fcf86", "DIAMOND" => "#7aa8ff", "MASTER" => "#c27cf0",
        "GRANDMASTER" => "#ff6b6b", "CHALLENGER" => "#f5d77a", _ => DIM,
    }
}

/// Scanning system fonts is slow; build the font database once per process.
fn fonts() -> Arc<fontdb::Database> {
    static FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db.load_font_data(include_bytes!("../assets/fonts/BeVietnamPro-Regular.ttf").to_vec());
        db.load_font_data(include_bytes!("../assets/fonts/BeVietnamPro-Bold.ttf").to_vec());
        Arc::new(db)
    }).clone()
}
/// Images are laid out on a 1200 px grid and drawn at twice that size.
pub(crate) const SCALE: f32 = 2.0;
/// Lossy WebP quality: text and icons stay crisp and a full scoreboard is a few hundred KB.
const WEBP_QUALITY: f32 = 90.0;
pub(crate) const IMAGE_EXT: &str = "webp";
/// Draws the SVG, laid out on a `width` x `height` grid, at `SCALE` and encodes it as WebP.
pub(crate) fn rasterize(svg: &str, width: u32, height: u32) -> Result<Vec<u8>> {
    let options = resvg::usvg::Options { font_family: "Be Vietnam Pro".into(), fontdb: fonts(), ..Default::default() };
    let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|_| anyhow::anyhow!("Cannot parse image SVG"))?;
    let (w, h) = ((width as f32 * SCALE) as u32, (height as f32 * SCALE) as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or_else(|| anyhow::anyhow!("Cannot allocate image"))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(SCALE, SCALE), &mut pixmap.as_mut());
    // Every image starts with an opaque background rectangle, so premultiplied and straight alpha agree.
    Ok(webp::Encoder::from_rgba(pixmap.data(), w, h).encode(WEBP_QUALITY).to_vec())
}
pub async fn blocking(work: impl FnOnce() -> Result<Vec<u8>> + Send + 'static) -> Result<Vec<u8>> {
    tokio::task::spawn_blocking(work).await.map_err(|_| anyhow::anyhow!("Image worker failed"))?
}

/// Approximate advance width for laying out inline chips (Be Vietnam Pro averages ~0.6em).
pub(crate) fn text_width(text: &str, size: u32) -> u32 { (text.chars().count() as f64 * f64::from(size) * 0.6).round() as u32 }
/// Label, ink and pill fill for a scoreboard badge.
pub(crate) fn badge_style(badge: Badge, lang: Lang) -> (&'static str, &'static str, &'static str) {
    match badge {
        Badge::Mvp => (lang.t(Msg::ImgMvp), GOLD, "#3d3418"),
        Badge::Ace => (lang.t(Msg::ImgAce), "#c4cee4", "#2a3242"),
        Badge::Worst => (lang.t(Msg::ImgWorst), RED, "#3d1d26"),
    }
}
pub async fn image(app: &App, game: Match, lang: Lang, preview: bool) -> Result<Vec<u8>> {
    let directory = app.cfg.database.parent().unwrap_or(Path::new(".")).join("artwork");
    let art = Assets::load(Wanted::from_match(&game), directory).await;
    let timezone = app.cfg.reports.timezone.clone();
    blocking(move || crate::scoreboard::scoreboard(&game, lang, &timezone, preview, &art)).await
}

/// Preview lookup does not import matches or enqueue historical notifications.
pub async fn preview_match(app: &App, match_id: Option<&str>) -> Result<Match> {
    if let Some(game) = app.store.get_match(match_id)? { return Ok(game); }
    let mut roster = app.store.players()?;
    for player in &mut roster {
        if player.puuid.is_empty() { player.puuid = app.riot.resolve(player).await?.unwrap_or_default(); }
    }
    if let Some(id) = match_id {
        ensure!(id.starts_with(&format!("{}_", app.cfg.riot.platform.to_uppercase())) && id.len() < 80 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "Match ID must use this configured platform prefix");
        let value = app.riot.match_data(id).await?.ok_or_else(|| anyhow::anyhow!("Requested match is not published"))?;
        return Match::from_riot(id, &value, &roster);
    }
    let mut ids = HashSet::new();
    for player in roster.iter().filter(|p| !p.puuid.is_empty()) {
        match app.riot.history(&player.puuid, 1).await {
            Ok(history) => ids.extend(history),
            Err(error) => tracing::warn!(%error, "Preview history lookup unavailable for one account"),
        }
    }
    // Match IDs grow over time, so only the newest one needs downloading.
    let id = ids.into_iter().max_by_key(|id| (id.len(), id.clone())).ok_or_else(|| anyhow::anyhow!("No published recent match is available for this roster"))?;
    let value = app.riot.match_data(&id).await?.ok_or_else(|| anyhow::anyhow!("Newest match is not published yet"))?;
    Match::from_riot(&id, &value, &roster)
}

/// Real recent matches for a report preview, fetched read-only and never stored.
pub async fn preview_matches(app: &App, start: i64, limit: usize) -> Result<Vec<Match>> {
    let roster = app.store.players()?;
    let mut ids = std::collections::BTreeSet::new();
    for player in roster.iter().filter(|p| !p.puuid.is_empty()) {
        match app.riot.history_since(&player.puuid, start, 100).await {
            Ok(history) => ids.extend(history),
            Err(error) => tracing::warn!(%error, "Preview history lookup unavailable for one account"),
        }
    }
    // Match IDs grow over time: keep the newest when the window is larger than the limit.
    let ids: Vec<String> = ids.into_iter().rev().take(limit).collect();
    // Finished matches never change; a private file cache makes repeated previews instant.
    let cache = app.cfg.database.parent().unwrap_or(Path::new(".")).join("preview-matches");
    std::fs::create_dir_all(&cache)?;
    let total = ids.len();
    tracing::info!(matches = total, "Loading recent matches for a report preview (not tracked)");
    let mut matches = Vec::new();
    for (index, id) in ids.into_iter().enumerate() {
        let file = cache.join(format!("{id}.json"));
        let cached = std::fs::read(&file).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        let fetched = match cached {
            Some(value) => Ok(Some(value)),
            None => {
                if index % 10 == 0 { tracing::info!(done = index, total, "Fetching preview matches from Riot"); }
                let value = app.riot.match_data(&id).await;
                if let Ok(Some(value)) = &value { let _ = std::fs::write(&file, value.to_string()); }
                value
            }
        };
        match fetched {
            Ok(Some(value)) => match Match::from_riot(&id, &value, &roster) {
                Ok(game) => matches.push(game),
                Err(error) => tracing::warn!(%error, "Skipping an unparseable preview match"),
            },
            Ok(None) => (),
            Err(error) => tracing::warn!(%error, "Preview match unavailable"),
        }
    }
    Ok(matches)
}
