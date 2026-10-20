//! Independently authored Rust scoreboard with public Data Dragon artwork.
use crate::{App, i18n::{Lang, Msg}, model::{Match, Participant}, presentation::{self, Badge, plain}, render_assets::{Assets, Wanted}};
use anyhow::{Result, ensure};
use resvg::usvg::fontdb;
use serde_json::Value;
use std::{collections::{BTreeMap, HashSet}, fmt::Write, path::Path, sync::{Arc, OnceLock}};

/// Splash darkening (left to right) and bottom fade shared by scoreboard and report heroes.
pub(crate) const HERO_DEFS: &str = "<defs><linearGradient id='heroShade'><stop stop-color='#0b1220' stop-opacity='.98'/><stop offset='.42' stop-color='#0b1220' stop-opacity='.8'/><stop offset='1' stop-color='#0b1220' stop-opacity='.2'/></linearGradient><linearGradient id='fade' x2='0' y2='1'><stop stop-color='#0b1220' stop-opacity='0'/><stop offset='1' stop-color='#0b1220'/></linearGradient></defs>";
pub(crate) const WIDTH: u32 = 1440;
pub(crate) const TEXT: &str = "#f0f1f7";
pub(crate) const DIM: &str = "#9ca8bb";
pub(crate) const FAINT: &str = "#69788e";
pub(crate) const GOLD: &str = "#edc976";
pub(crate) const GREEN: &str = "#6cdbb0";
pub(crate) const RED: &str = "#fa8297";
pub(crate) const BLUE: &str = "#72b8fa";

pub(crate) struct Canvas { pub svg: String, pub clips: usize }
impl Canvas {
    pub fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, r: u32, fill: &str) {
        let _ = write!(self.svg, "<rect x='{x}' y='{y}' width='{w}' height='{h}' rx='{r}' fill='{fill}'/>");
    }
    pub fn outline(&mut self, x: u32, y: u32, w: u32, h: u32, r: u32, color: &str) {
        let _ = write!(self.svg, "<rect x='{x}' y='{y}' width='{w}' height='{h}' rx='{r}' fill='none' stroke='{color}' stroke-width='1.5'/>");
    }
    pub fn text(&mut self, x: u32, y: u32, size: u32, fill: &str, weight: u16, anchor: &str, text: &str) {
        let _ = write!(self.svg, "<text x='{x}' y='{y}' font-size='{size}' fill='{fill}' font-weight='{weight}' text-anchor='{anchor}'>{}</text>", escape(text));
    }
    pub fn image(&mut self, data: Option<&str>, x: u32, y: u32, w: u32, h: u32, radius: u32) {
        let Some(data) = data else { self.rect(x, y, w, h, radius, "#27364b"); return; };
        self.clips += 1;
        let id = self.clips;
        let _ = write!(self.svg, "<defs><clipPath id='c{id}'><rect x='{x}' y='{y}' width='{w}' height='{h}' rx='{radius}'/></clipPath></defs><image href='{data}' x='{x}' y='{y}' width='{w}' height='{h}' preserveAspectRatio='xMidYMid slice' clip-path='url(#c{id})'/>");
    }
}
fn escape(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).map(|c| match c {
        '&' => "&amp;".into(), '<' => "&lt;".into(), '>' => "&gt;".into(), '\'' => "&apos;".into(), '"' => "&quot;".into(), _ => c.to_string(),
    }).collect()
}
pub(crate) fn short(text: &str, count: usize) -> String {
    let text = plain(text);
    if text.chars().count() <= count { text } else { format!("{}…", text.chars().take(count.saturating_sub(1)).collect::<String>()) }
}
pub(crate) fn compact(value: i64) -> String { if value >= 1000 { format!("{:.1}k", value as f64 / 1000.0) } else { value.to_string() } }
pub(crate) fn number(value: i64) -> String {
    let digits = value.to_string();
    digits.chars().rev().enumerate().flat_map(|(i, c)| if i > 0 && i % 3 == 0 { vec![',', c] } else { vec![c] }).collect::<Vec<_>>().into_iter().rev().collect()
}
fn color(team: u16) -> &'static str { if team == 100 { BLUE } else { RED } }
fn team_name(team: u16, lang: Lang) -> String { lang.team(team).to_uppercase() }

const HERO_H: u32 = 304;
const ROW_H: u32 = 78;
const ROW_GAP: u32 = 8;
const TEAM_H: u32 = 62;
/// Row content spans this margin on both sides of the page.
const MARGIN: u32 = 32;
/// Objective icon name and the Match-V5 key whose count it shows.
const OBJECTIVE_KINDS: [(&str, &str); 5] = [("dragon", "dragon"), ("baron", "baron"), ("herald", "riftHerald"), ("tower", "tower"), ("inhibitor", "inhibitor")];

pub(crate) fn tier_color(tier: &str) -> &'static str {
    match tier {
        "IRON" => "#9a9a9a", "BRONZE" => "#c58b65", "SILVER" => "#b8c4d0", "GOLD" => "#e6c06a",
        "PLATINUM" => "#4fc1b8", "EMERALD" => "#3fcf86", "DIAMOND" => "#7aa8ff", "MASTER" => "#c27cf0",
        "GRANDMASTER" => "#ff6b6b", "CHALLENGER" => "#f5d77a", _ => DIM,
    }
}

pub fn scoreboard(game: &Match, lang: Lang, timezone: &str, preview: bool, art: &Assets) -> Result<Vec<u8>> {
    ensure!(!game.participants.is_empty() && game.participants.len() <= 32, "Unsupported scoreboard size");
    let mut grouped: BTreeMap<u16, Vec<&Participant>> = BTreeMap::new();
    for player in &game.participants { grouped.entry(player.team).or_default().push(player); }
    let mut teams: Vec<_> = grouped.into_iter().collect();
    teams.sort_by_key(|(id, players)| (!players.iter().any(|p| p.tracked), *id));
    let height = HERO_H + teams.iter().map(|(_, rows)| TEAM_H + (ROW_H + ROW_GAP) * rows.len() as u32 + 18).sum::<u32>() + 8;
    ensure!(height <= 5000, "Scoreboard exceeds image bounds");
    let mut cv = Canvas { clips: 0, svg: format!("<svg xmlns='http://www.w3.org/2000/svg' width='{WIDTH}' height='{height}'>") };
    cv.svg.push_str(HERO_DEFS);
    cv.svg.push_str(SCOREBOARD_DEFS);
    cv.svg.push_str("<g font-family='Be Vietnam Pro,DejaVu Sans,sans-serif'>");
    cv.rect(0, 0, WIDTH, height, 0, "url(#page)");
    let badges = presentation::badges(game);
    if let Some(splash) = art.image("splash") {
        let _ = write!(cv.svg, "<defs><clipPath id='hero'><rect width='{WIDTH}' height='{HERO_H}'/></clipPath></defs><image href='{splash}' width='{WIDTH}' height='{HERO_H}' preserveAspectRatio='xMidYMin slice' clip-path='url(#hero)'/>");
    } else { cv.rect(0, 0, WIDTH, HERO_H, 0, "#182334"); }
    cv.rect(0, 0, WIDTH, HERO_H, 0, "url(#heroShade)");
    cv.rect(0, HERO_H - 130, WIDTH, 130, 0, "url(#fade)");
    let tracked: Vec<_> = game.participants.iter().filter(|p| p.tracked).collect();
    let split = tracked.iter().any(|p| tracked.first().is_some_and(|f| f.team != p.team));
    let won = teams.first().is_some_and(|(_, rows)| rows.first().is_some_and(|p| p.win));
    let result = lang.t(if game.remake { Msg::ImgRemake } else if split { Msg::ImgResult } else if won { Msg::ImgVictory } else { Msg::ImgDefeat });
    let accent = if game.remake || split { DIM } else if won { GREEN } else { RED };
    cv.rect(0, 36, 5, 156, 0, accent);
    cv.text(54, 56, 13, GOLD, 700, "start", lang.t(Msg::ImgHeader));
    cv.text(50, 136, 72, accent, 700, "start", result);
    cv.text(54, 178, 24, TEXT, 700, "start", &format!("{}  ·  {}:{:02}", lang.queue(game.queue), game.duration / 60, game.duration % 60));
    let tz = timezone.parse::<chrono_tz::Tz>().map_err(|_| anyhow::anyhow!("Invalid image timezone"))?;
    let ended = chrono::DateTime::from_timestamp(game.ended_at, 0).ok_or_else(|| anyhow::anyhow!("Invalid image timestamp"))?.with_timezone(&tz);
    cv.text(54, 206, 15, DIM, 400, "start", &ended.format(lang.t(Msg::ImgDateFormat)).to_string());
    if preview { cv.text(WIDTH - 32, 29, 11, DIM, 400, "end", lang.t(Msg::ImgPreview)); }
    if teams.len() >= 2 {
        let left = WIDTH - 404;
        cv.rect(left, 44, 356, 140, 24, "rgba(8,14,25,.82)");
        cv.outline(left, 44, 356, 140, 24, "#39465a");
        for (index, (id, players)) in teams.iter().take(2).enumerate() {
            let x = left + 96 + index as u32 * 164;
            cv.text(x, 80, 12, color(*id), 700, "middle", &team_name(*id, lang));
            cv.text(x, 152, 66, color(*id), 700, "middle", &players.iter().map(|p| p.kills).sum::<i64>().to_string());
        }
        cv.text(left + 178, 142, 13, FAINT, 700, "middle", "VS");
        draw_strip(&mut cv, game, &teams, lang, art);
    }
    let best_damage = game.participants.iter().map(|p| p.damage).max().unwrap_or(0);
    let best_gold = game.participants.iter().map(|p| p.gold).max().unwrap_or(0);
    let mut top = HERO_H;
    for (team, mut players) in teams {
        players.sort_by_key(|p| p.id);
        draw_team(&mut cv, game, team, &players, top, lang, art, &badges, best_damage, best_gold);
        top += TEAM_H + (ROW_H + ROW_GAP) * players.len() as u32 + 18;
    }
    cv.svg.push_str("</g></svg>");
    rasterize(&cv.svg, WIDTH, height)
}

/// One glass strip under the title: team gold at both ends, objective icons with each team's count between.
fn draw_strip(cv: &mut Canvas, game: &Match, teams: &[(u16, Vec<&Participant>)], lang: Lang, art: &Assets) {
    let (top, strip_h) = (230, 58);
    cv.rect(MARGIN, top, WIDTH - 2 * MARGIN, strip_h, 20, "rgba(8,14,25,.80)");
    cv.outline(MARGIN, top, WIDTH - 2 * MARGIN, strip_h, 20, "#2b3a52");
    let gold = |index: usize| teams[index].1.iter().map(|p| p.gold).sum::<i64>();
    let (a, b) = (gold(0), gold(1));
    cv.text(MARGIN + 24, top + 22, 10, FAINT, 700, "start", lang.t(Msg::ImgGold));
    cv.text(MARGIN + 24, top + 45, 22, color(teams[0].0), 700, "start", &compact(a));
    cv.text(WIDTH - MARGIN - 24, top + 22, 10, FAINT, 700, "end", lang.t(Msg::ImgGold));
    cv.text(WIDTH - MARGIN - 24, top + 45, 22, color(teams[1].0), 700, "end", &compact(b));
    // The lead as a hairline along the bottom of the strip.
    let inner = WIDTH - 2 * MARGIN - 40;
    let share = (a as f64 / (a + b).max(1) as f64 * f64::from(inner)).clamp(0.0, f64::from(inner)) as u32;
    cv.rect(MARGIN + 20, top + strip_h - 6, inner, 3, 1, color(teams[1].0));
    cv.rect(MARGIN + 20, top + strip_h - 6, share, 3, 1, color(teams[0].0));
    let count = |team: u16, key: &str| game.objectives.get(&team.to_string()).and_then(|o| o.get(key)).copied();
    let kinds: Vec<_> = OBJECTIVE_KINDS.iter().filter(|(_, key)| teams.iter().take(2).any(|(id, _)| count(*id, key).is_some())).collect();
    // Each objective is one pale icon with the blue-side count on its left and the red-side count on its right.
    let pitch = 150;
    for index in 1..kinds.len() as u32 {
        cv.rect(WIDTH / 2 + index * pitch - kinds.len() as u32 * pitch / 2, top + 16, 1, 26, 0, "#2b3a52");
    }
    for (index, (icon, key)) in kinds.iter().enumerate() {
        let cx = WIDTH / 2 + index as u32 * pitch - (kinds.len() as u32 - 1) * pitch / 2;
        let values: Vec<i64> = teams.iter().take(2).map(|(id, _)| count(*id, key).unwrap_or(0)).collect();
        let quiet = values.iter().all(|v| *v == 0);
        let _ = write!(cv.svg, "<g opacity='{}'>", if quiet { ".3" } else { ".95" });
        if let Some(data) = art.image(&format!("objective:{icon}-100")) {
            let _ = write!(cv.svg, "<image href='{data}' x='{}' y='{}' width='30' height='30' filter='url(#mono)'/>", cx - 15, top + 14);
        }
        cv.svg.push_str("</g>");
        for (side, ((id, _), value)) in teams.iter().take(2).zip(&values).enumerate() {
            let (x, anchor) = if side == 0 { (cx - 26, "end") } else { (cx + 26, "start") };
            cv.text(x, top + 37, 21, if *value > 0 { color(*id) } else { FAINT }, 700, anchor, &value.to_string());
        }
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
pub(crate) fn rasterize(svg: &str, width: u32, height: u32) -> Result<Vec<u8>> {
    let options = resvg::usvg::Options { font_family: "Be Vietnam Pro".into(), fontdb: fonts(), ..Default::default() };
    let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|_| anyhow::anyhow!("Cannot parse image SVG"))?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| anyhow::anyhow!("Cannot allocate image"))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|_| anyhow::anyhow!("Cannot encode PNG"))
}
pub async fn blocking(work: impl FnOnce() -> Result<Vec<u8>> + Send + 'static) -> Result<Vec<u8>> {
    tokio::task::spawn_blocking(work).await.map_err(|_| anyhow::anyhow!("Image worker failed"))?
}

/// Page gradient, tracked-row gradients, header rules and the row shadow.
const SCOREBOARD_DEFS: &str = "<defs>\
<linearGradient id='page' x2='0' y2='1'><stop stop-color='#0e1628'/><stop offset='1' stop-color='#080d18'/></linearGradient>\
<linearGradient id='row100'><stop stop-color='#1f3c60'/><stop offset='.55' stop-color='#152a43'/><stop offset='1' stop-color='#111c2e'/></linearGradient>\
<linearGradient id='row200'><stop stop-color='#47263e'/><stop offset='.55' stop-color='#2c1f30'/><stop offset='1' stop-color='#181725'/></linearGradient>\
<filter id='mono'><feColorMatrix type='matrix' values='.3 1 .1 0 0  .3 1 .1 0 0  .3 1 .1 0 0  0 0 0 1 0'/></filter>\
<linearGradient id='rule100'><stop stop-color='#72b8fa'/><stop offset='1' stop-color='#72b8fa' stop-opacity='0'/></linearGradient>\
<linearGradient id='rule200'><stop stop-color='#fa8297'/><stop offset='1' stop-color='#fa8297' stop-opacity='0'/></linearGradient>\
<filter id='shadow' x='-5%' y='-30%' width='110%' height='170%'><feDropShadow dx='0' dy='6' stdDeviation='8' flood-color='#000' flood-opacity='.45'/></filter>\
</defs>";

/// Approximate advance width for laying out inline chips (Be Vietnam Pro averages ~0.6em).
fn text_width(text: &str, size: u32) -> u32 { (text.chars().count() as f64 * f64::from(size) * 0.6).round() as u32 }
/// Rounded label chip; returns its width.
fn pill(cv: &mut Canvas, x: u32, y: u32, height: u32, fill: &str, color: &str, size: u32, text: &str) -> u32 {
    let width = text_width(text, size) + height;
    cv.rect(x, y, width, height, height / 2, fill);
    cv.text(x + width / 2, y + height / 2 + size * 7 / 20, size, color, 700, "middle", text);
    width
}

/// Label, ink and pill fill for a scoreboard badge.
fn badge_style(badge: Badge, lang: Lang) -> (&'static str, &'static str, &'static str) {
    match badge {
        Badge::Mvp => (lang.t(Msg::ImgMvp), GOLD, "#3d3418"),
        Badge::Worst => (lang.t(Msg::ImgWorst), RED, "#3d1d26"),
    }
}
// Column centres, the damage column's left edge and the first item slot.
const KDA_X: u32 = 510;
const DAMAGE_X: u32 = 612;
const DAMAGE_W: u32 = 184;
const TAKEN_X: u32 = 858;
const CS_X: u32 = 946;
const GOLD_X: u32 = 1030;
const VISION_X: u32 = 1106;
const ITEMS_X: u32 = 1150;
const ITEM_PITCH: u32 = 33;

fn draw_team(cv: &mut Canvas, game: &Match, team: u16, players: &[&Participant], top: u32, lang: Lang, art: &Assets, badges: &[(u64, Badge)], best_damage: i64, best_gold: i64) {
    let tint = color(team);
    let won = players.first().is_some_and(|p| p.win);
    let total_damage: i64 = players.iter().map(|p| p.damage).sum();
    // Open header: team name, result chip, quiet column labels and a fading team rule.
    let name = team_name(team, lang);
    cv.rect(MARGIN + 2, top + 16, 5, 26, 2, tint);
    cv.text(MARGIN + 18, top + 38, 22, tint, 700, "start", &name);
    let result = lang.t(if game.remake { Msg::ImgRemake } else if won { Msg::ImgWin } else { Msg::ImgLoss });
    pill(cv, MARGIN + 18 + text_width(&name, 22) + 22, top + 14, 28, if won { "#1c4a42" } else { "#4d2635" }, if won { GREEN } else { RED }, 12, result);
    for (x, label) in [(KDA_X, "KDA"), (DAMAGE_X + DAMAGE_W / 2, lang.t(Msg::ImgDamage)), (TAKEN_X, lang.t(Msg::ImgTaken)), (CS_X, "CS"), (GOLD_X, lang.t(Msg::ImgGold)), (VISION_X, lang.t(Msg::ImgVision)), (ITEMS_X + 113, lang.t(Msg::ImgItems))] {
        cv.text(x, top + 38, 11, FAINT, 700, "middle", label);
    }
    let _ = write!(cv.svg, "<rect x='{MARGIN}' y='{}' width='{}' height='2' fill='url(#rule{team})'/>", top + 48, WIDTH - 2 * MARGIN);
    let row_w = WIDTH - 2 * MARGIN;
    for (index, p) in players.iter().enumerate() {
        let y = top + TEAM_H + index as u32 * (ROW_H + ROW_GAP);
        let badge = badges.iter().find(|(id, _)| *id == p.id).map(|(_, b)| badge_style(*b, lang));
        // Tracked players stand out; everyone else sits on a quiet flat row.
        if p.tracked {
            let _ = write!(cv.svg, "<rect x='{MARGIN}' y='{y}' width='{row_w}' height='{ROW_H}' rx='18' fill='url(#row{team})'/>");
            cv.outline(MARGIN, y, row_w, ROW_H, 18, if team == 100 { "rgba(114,184,250,.5)" } else { "rgba(250,130,151,.5)" });
        } else {
            cv.rect(MARGIN, y, row_w, ROW_H, 18, "#0f1727");
        }
        let ink = badge.map_or(if p.tracked { tint } else { "#2c3a50" }, |(_, ink, _)| ink);
        cv.image(art.image(&format!("champion:{}", p.champion)), 52, y + 11, 56, 56, 15);
        cv.outline(52, y + 11, 56, 56, 15, ink);
        // MVP / worst-player label sits across the top edge of the portrait.
        if let Some((label, ink, fill)) = badge {
            let width = text_width(label, 10) + 20;
            cv.rect(80 - width / 2, y + 2, width, 18, 9, "#0b1220");
            pill(cv, 80 - width / 2, y + 2, 18, fill, ink, 10, label);
        }
        if p.level > 0 {
            cv.rect(86, y + 52, 28, 19, 9, "#0b1220");
            cv.text(100, y + 66, 11, TEXT, 700, "middle", &p.level.to_string());
        }
        let (name_size, name_color, weight) = if p.tracked { (21, TEXT, 700) } else { (18, "#aab4c4", 400) };
        cv.text(128, y + 34, name_size, name_color, weight, "start", &short(&p.name, 22));
        let mut x = 128;
        let champion = art.names.get(&p.champion).map(String::as_str).unwrap_or(&p.champion);
        let secondary = match (&p.rank_after, lang.role(&p.role)) {
            (Some(rank), _) if p.tracked => {
                // A small tier gem in the tier's colour stands in for the rank emblem.
                let (gx, gy) = (x + 6, y + 55);
                let _ = write!(cv.svg, "<polygon points='{gx},{} {},{gy} {gx},{} {},{gy}' fill='{}'/>", gy - 6, gx + 6, gy + 6, gx - 6, tier_color(&rank.tier));
                x += 18;
                lang.rank(rank)
            }
            (_, "") => champion.to_owned(),
            (_, role) => format!("{champion} · {role}"),
        };
        let secondary = short(&secondary, 26);
        cv.text(x, y + 60, 13, DIM, 400, "start", &secondary);
        if let (true, Some(delta)) = (p.tracked, p.lp_delta) {
            let (fill, ink) = if delta > 0 { ("#183f37", GREEN) } else { ("#4a2534", RED) };
            // text_width over-estimates 13px body text by about an eighth.
            pill(cv, x + text_width(&secondary, 13) * 7 / 8 + 8, y + 44, 20, fill, ink, 10, &format!("{delta:+} LP"));
        }
        for (offset, value, c) in [(-46i32, p.kills, TEXT), (0, p.deaths, RED), (46, p.assists, BLUE)] {
            cv.text((KDA_X as i32 + offset) as u32, y + 36, 25, c, 700, "middle", &value.to_string());
        }
        cv.text(KDA_X - 23, y + 35, 18, FAINT, 400, "middle", "/");
        cv.text(KDA_X + 23, y + 35, 18, FAINT, 400, "middle", "/");
        let ratio = (p.kills + p.assists) as f64 / p.deaths.max(1) as f64;
        let (ratio_fill, ratio_color) = if ratio >= 4.0 { ("#183f37", GREEN) } else if ratio < 1.0 { ("#4a2534", RED) } else { ("#222e40", DIM) };
        cv.rect(KDA_X - 27, y + 47, 54, 21, 10, ratio_fill);
        cv.text(KDA_X, y + 62, 12, ratio_color, 700, "middle", &format!("{ratio:.1}"));
        // Damage: the number, its team share, and a bar scaled to the game's top damage dealer.
        let top_damage = p.damage == best_damage && best_damage > 0;
        cv.text(DAMAGE_X, y + 37, 22, if top_damage { GOLD } else { TEXT }, 700, "start", &number(p.damage));
        cv.text(DAMAGE_X + DAMAGE_W, y + 37, 13, FAINT, 400, "end", &format!("{:.0}%", p.damage as f64 / total_damage.max(1) as f64 * 100.0));
        cv.rect(DAMAGE_X, y + 49, DAMAGE_W, 8, 4, "#223048");
        cv.rect(DAMAGE_X, y + 49, (p.damage as f64 / best_damage.max(1) as f64 * f64::from(DAMAGE_W)).clamp(4.0, f64::from(DAMAGE_W)) as u32, 8, 4, if top_damage { GOLD } else { tint });
        cv.text(TAKEN_X, y + 45, 20, "#c9d1de", 400, "middle", &number(p.damage_taken));
        cv.text(CS_X, y + 46, 22, TEXT, 700, "middle", &p.cs.to_string());
        cv.text(GOLD_X, y + 46, 22, if p.gold == best_gold { GOLD } else { TEXT }, 700, "middle", &compact(p.gold));
        cv.text(VISION_X, y + 45, 20, "#c9d1de", 400, "middle", &p.vision.to_string());
        for (slot, item) in p.items.iter().take(7).enumerate() {
            let x = ITEMS_X + slot as u32 * ITEM_PITCH;
            let radius = if slot == 6 { 14 } else { 7 };
            if *item > 0 { cv.image(art.image(&format!("item:{item}")), x, y + 24, 29, 29, radius); } else { cv.rect(x, y + 24, 29, 29, radius, "#0d1422"); }
            cv.outline(x, y + 24, 29, 29, radius, "#2a3446");
        }
    }
}

pub async fn png(app: &App, game: Match, lang: Lang, preview: bool) -> Result<Vec<u8>> {
    let directory = app.cfg.database.parent().unwrap_or(Path::new(".")).join("artwork");
    let art = Assets::load(Wanted::from_match(&game), directory).await;
    let timezone = app.cfg.reports.timezone.clone();
    blocking(move || scoreboard(&game, lang, &timezone, preview, &art)).await
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
