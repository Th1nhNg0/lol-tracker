//! Independently authored Rust scoreboard with public Data Dragon artwork.
use crate::{App, i18n::{Lang, Msg}, model::{Match, Participant, n}, presentation::{self, Badge, plain}, render_assets::{Assets, Wanted}};
use anyhow::{Result, ensure};
use resvg::usvg::fontdb;
use serde_json::Value;
use std::{collections::{BTreeMap, HashSet}, fmt::Write, path::Path, sync::{Arc, OnceLock}};

/// Splash darkening (left to right) and bottom fade shared by scoreboard and report heroes.
pub(crate) const HERO_DEFS: &str = "<defs><linearGradient id='heroShade'><stop stop-color='#0b1220' stop-opacity='.98'/><stop offset='.42' stop-color='#0b1220' stop-opacity='.8'/><stop offset='1' stop-color='#0b1220' stop-opacity='.2'/></linearGradient><linearGradient id='fade' x2='0' y2='1'><stop stop-color='#0b1220' stop-opacity='0'/><stop offset='1' stop-color='#0b1220'/></linearGradient></defs>";
pub(crate) const WIDTH: u32 = 1600;
const ROW: u32 = 80;
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

pub fn scoreboard(game: &Match, lang: Lang, timezone: &str, preview: bool, art: &Assets) -> Result<Vec<u8>> {
    ensure!(!game.participants.is_empty() && game.participants.len() <= 32, "Unsupported scoreboard size");
    let mut grouped: BTreeMap<u16, Vec<&Participant>> = BTreeMap::new();
    for player in &game.participants { grouped.entry(player.team).or_default().push(player); }
    let mut teams: Vec<_> = grouped.into_iter().collect();
    teams.sort_by_key(|(id, players)| (!players.iter().any(|p| p.tracked), *id));
    let height = 350 + teams.iter().map(|(_, rows)| 106 + ROW * rows.len() as u32).sum::<u32>() + 48;
    ensure!(height <= 5000, "Scoreboard exceeds image bounds");
    let mut cv = Canvas { clips: 0, svg: format!("<svg xmlns='http://www.w3.org/2000/svg' width='{WIDTH}' height='{height}'>") };
    cv.svg.push_str(HERO_DEFS);
    cv.svg.push_str(SCOREBOARD_DEFS);
    cv.svg.push_str("<g font-family='Be Vietnam Pro,DejaVu Sans,sans-serif'>");
    cv.rect(0, 0, WIDTH, height, 0, "url(#page)");
    let badges = presentation::badges(game);
    if let Some(splash) = art.image("splash") {
        let _ = write!(cv.svg, "<defs><clipPath id='hero'><rect width='{WIDTH}' height='324'/></clipPath></defs><image href='{splash}' width='{WIDTH}' height='324' preserveAspectRatio='xMidYMin slice' clip-path='url(#hero)'/>");
    } else { cv.rect(0, 0, WIDTH, 324, 0, "#182334"); }
    cv.rect(0, 0, WIDTH, 324, 0, "url(#heroShade)");
    cv.rect(0, 200, WIDTH, 124, 0, "url(#fade)");
    let tracked: Vec<_> = game.participants.iter().filter(|p| p.tracked).collect();
    let split = tracked.iter().any(|p| tracked.first().is_some_and(|f| f.team != p.team));
    let focus_team = teams.first().map(|(id, _)| *id).unwrap_or(100);
    let won = teams.first().is_some_and(|(_, rows)| rows.first().is_some_and(|p| p.win));
    let result = lang.t(if game.remake { Msg::ImgRemake } else if split { Msg::ImgResult } else if won { Msg::ImgVictory } else { Msg::ImgDefeat });
    let accent = if game.remake || split { DIM } else if won { GREEN } else { RED };
    cv.rect(0, 32, 5, 225, 0, accent);
    cv.text(54, 44, 13, GOLD, 700, "start", lang.t(Msg::ImgHeader));
    cv.text(50, 119, 60, accent, 700, "start", result);
    cv.text(54, 159, 22, TEXT, 700, "start", &format!("{}  ·  {}:{:02}", lang.queue(game.queue), game.duration / 60, game.duration % 60));
    let tz = timezone.parse::<chrono_tz::Tz>().map_err(|_| anyhow::anyhow!("Invalid image timezone"))?;
    let ended = chrono::DateTime::from_timestamp(game.ended_at, 0).ok_or_else(|| anyhow::anyhow!("Invalid image timestamp"))?.with_timezone(&tz);
    cv.text(54, 187, 15, DIM, 400, "start", &ended.format(lang.t(Msg::ImgDateFormat)).to_string());
    for (i, p) in tracked.iter().take(10).enumerate() {
        let x = 54 + i as u32 * 43;
        cv.image(art.image(&format!("champion:{}", p.champion)), x, 207, 47, 47, 23);
        cv.outline(x, 207, 47, 47, 23, color(p.team));
    }
    let group_x = 54 + tracked.len().min(10) as u32 * 43 + 21;
    cv.text(group_x, 235, 16, TEXT, 700, "start", &lang.f(Msg::ImgTrackedPlayers, &[&tracked.len()]));
    if preview { cv.text(1542, 29, 11, DIM, 400, "end", lang.t(Msg::ImgPreview)); }
    if teams.len() >= 2 {
        cv.rect(1142, 52, 402, 139, 20, "rgba(8,14,25,.84)");
        cv.outline(1142, 52, 402, 139, 20, "#39465a");
        for (index, (id, players)) in teams.iter().take(2).enumerate() {
            let x = 1243 + index as u32 * 202;
            cv.text(x, 83, 13, color(*id), 700, "middle", &team_name(*id, lang));
            cv.text(x, 156, 63, color(*id), 700, "middle", &players.iter().map(|p| p.kills).sum::<i64>().to_string());
        }
        cv.text(1344, 128, 14, FAINT, 700, "middle", "VS");
        // The tracked team's MVP/ACE (same deterministic score as the text verdict).
        let star = game.participants.iter().find_map(|p| badges.iter().find(|(id, b)| *id == p.id && p.team == focus_team && *b != Badge::Worst).map(|(_, b)| (p, *b)));
        if let Some((star, badge)) = star {
            let (label, ink, _) = badge_style(badge, lang);
            cv.rect(1142, 203, 402, 62, 16, "rgba(8,14,25,.84)");
            cv.outline(1142, 203, 402, 62, 16, "#4a4f5c");
            cv.image(art.image(&format!("champion:{}", star.champion)), 1156, 211, 46, 46, 12);
            cv.outline(1156, 211, 46, 46, 12, ink);
            cv.text(1216, 229, 11, ink, 700, "start", label);
            cv.text(1216, 252, 17, TEXT, 700, "start", &format!("{}  ·  {}/{}/{}", short(&star.name, 20), star.kills, star.deaths, star.assists));
        }
        let a: i64 = teams[0].1.iter().map(|p| p.gold).sum();
        let b: i64 = teams[1].1.iter().map(|p| p.gold).sum();
        let share = (a as f64 / (a + b).max(1) as f64 * 1492.0).clamp(0.0, 1492.0) as u32;
        cv.rect(54, 281, 1492, 8, 4, color(teams[1].0));
        cv.rect(54, 281, share, 8, 4, color(focus_team));
        cv.text(54, 312, 16, color(teams[0].0), 700, "start", &compact(a));
        cv.text(1546, 312, 16, color(teams[1].0), 700, "end", &compact(b));
        cv.text(800, 312, 13, if a >= b { color(teams[0].0) } else { color(teams[1].0) }, 700, "middle", &format!("{}  ·  +{}", lang.t(Msg::ImgGoldDifference), compact((a - b).abs())));
    }
    let mut top = 350;
    for (team, mut players) in teams {
        players.sort_by_key(|p| p.id);
        draw_team(&mut cv, game, team, &players, top, lang, art, &badges);
        top += 106 + ROW * players.len() as u32;
    }
    cv.text(48, height - 19, 12, FAINT, 400, "start", &format!("{}  ·  Data Dragon {}", game.id, art.version));
    cv.text(1552, height - 19, 12, FAINT, 400, "end", lang.t(Msg::ImgFooter));
    cv.svg.push_str("</g></svg>");
    rasterize(&cv.svg, WIDTH, height)
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
fn chart_canvas(width: u32, height: u32, title: &str) -> Canvas {
    let mut cv = Canvas { clips: 0, svg: format!("<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='{height}'><g font-family='Be Vietnam Pro,DejaVu Sans,sans-serif'>") };
    cv.rect(0, 0, width, height, 0, "#0b1220");
    cv.text(32, 44, 22, TEXT, 700, "start", title);
    cv
}

const PHYSICAL: &str = "#f4a259";
const MAGIC: &str = "#a78bfa";
const TRUE_DAMAGE: &str = "#e8ecf3";

/// Champion damage per player, split into physical/magic/true, grouped by team.
pub fn damage_chart(game: &Match, lang: Lang) -> Result<Vec<u8>> {
    ensure!(!game.participants.is_empty() && game.participants.len() <= 32, "Unsupported chart size");
    let mut rows: Vec<&Participant> = game.participants.iter().collect();
    rows.sort_by_key(|p| (p.team, -p.damage));
    let (width, top, row) = (1000, 96, 40);
    let height = top + rows.len() as u32 * row + 40;
    let mut cv = chart_canvas(width, height, &format!("{} · {}", lang.t(Msg::ChartDamageTitle), lang.queue(game.queue)));
    for (i, (color, label)) in [(PHYSICAL, lang.t(Msg::ChartPhysical)), (MAGIC, lang.t(Msg::ChartMagic)), (TRUE_DAMAGE, lang.t(Msg::ChartTrue))].iter().enumerate() {
        let x = 32 + i as u32 * 130;
        cv.rect(x, 62, 14, 14, 3, color);
        cv.text(x + 22, 74, 13, DIM, 400, "start", label);
    }
    let best = rows.iter().map(|p| p.damage).max().unwrap_or(0).max(1) as f64;
    let (bar_x, bar_w) = (330.0, 560.0);
    for (i, p) in rows.iter().enumerate() {
        let y = top + i as u32 * row;
        if i > 0 && rows[i - 1].team != p.team { cv.rect(32, y - 3, width - 64, 1, 0, "#2c3a50"); }
        // The bundled font has no star glyph; mark tracked players with a dot instead.
        if p.tracked { cv.rect(32, y + 15, 10, 10, 5, GOLD); }
        let name = short(&p.name, 18);
        cv.text(50, y + 25, 15, color(p.team), if p.tracked { 700 } else { 400 }, "start", &name);
        cv.text(320, y + 25, 13, DIM, 400, "end", &short(&p.champion, 14));
        let mut x = bar_x;
        for (value, fill) in [(p.physical, PHYSICAL), (p.magic, MAGIC), (p.true_damage, TRUE_DAMAGE)] {
            let w = (value.max(0) as f64 / best * bar_w).round();
            if w >= 1.0 { cv.rect(x as u32, y + 10, w as u32, 20, 3, fill); }
            x += w;
        }
        cv.text(x as u32 + 8, y + 25, 14, TEXT, 700, "start", &number(p.damage));
    }
    cv.svg.push_str("</g></svg>");
    rasterize(&cv.svg, width, height)
}

/// Blue-minus-red team gold over time from the Match-V5 timeline.
pub fn gold_chart(game: &Match, timeline: &Value, lang: Lang) -> Result<Vec<u8>> {
    let teams: HashSet<u16> = game.participants.iter().map(|p| p.team).collect();
    ensure!(teams == HashSet::from([100, 200]), "Gold chart needs two standard teams");
    let points: Vec<(f64, i64)> = timeline["info"]["frames"].as_array().into_iter().flatten().map(|frame| {
        let diff = game.participants.iter().map(|p| {
            let gold = n(&frame["participantFrames"][p.id.to_string()], "totalGold");
            if p.team == 100 { gold } else { -gold }
        }).sum();
        (n(frame, "timestamp") as f64 / 60000.0, diff)
    }).collect();
    ensure!(points.len() >= 2, "Timeline has too few frames");
    let (width, height) = (1000u32, 480u32);
    let (left, right, top, bottom) = (90.0, 960.0, 90.0, 430.0);
    let span = points.iter().map(|(_, d)| d.abs()).max().unwrap_or(0).max(1000);
    let span = ((span + 999) / 1000 * 1000) as f64;
    let minutes = points.last().map_or(1.0, |(t, _)| t.max(1.0));
    let x = |t: f64| left + t / minutes * (right - left);
    let y = |d: f64| top + (span - d) / (2.0 * span) * (bottom - top);
    let zero = y(0.0);
    let tracked: Vec<u16> = game.participants.iter().filter(|p| p.tracked).map(|p| p.team).collect();
    let note = match (tracked.contains(&100), tracked.contains(&200)) {
        (true, false) => format!(" · {}", lang.f(Msg::ChartTrackedTeam, &[&lang.team(100).to_lowercase()])),
        (false, true) => format!(" · {}", lang.f(Msg::ChartTrackedTeam, &[&lang.team(200).to_lowercase()])),
        _ => String::new(),
    };
    let mut cv = chart_canvas(width, height, &format!("{}{note}", lang.t(Msg::ChartGoldTitle)));
    for step in [-1.0, -0.5, 0.0, 0.5, 1.0] {
        let value = span * step;
        let line_y = y(value) as u32;
        cv.rect(left as u32, line_y, (right - left) as u32, 1, 0, if step == 0.0 { "#5b6b84" } else { "#1f2a3c" });
        let label = if value > 0.0 { format!("+{}", compact(value as i64)) } else if value < 0.0 { format!("-{}", compact(-value as i64)) } else { "0".into() };
        cv.text(left as u32 - 10, line_y + 5, 12, if value > 0.0 { BLUE } else if value < 0.0 { RED } else { DIM }, 400, "end", &label);
    }
    let mut minute = 0;
    while f64::from(minute) <= minutes {
        cv.text(x(f64::from(minute)) as u32, bottom as u32 + 24, 12, DIM, 400, "middle", &format!("{minute}'"));
        minute += 5;
    }
    let path: String = points.iter().map(|(t, d)| format!("{:.1},{:.1}", x(*t), y(*d as f64))).collect::<Vec<_>>().join(" ");
    let area = format!("{:.1},{zero:.1} {path} {:.1},{zero:.1}", x(points[0].0), x(minutes));
    let _ = write!(cv.svg, "<defs><clipPath id='above'><rect x='0' y='0' width='{width}' height='{zero:.1}'/></clipPath><clipPath id='below'><rect x='0' y='{zero:.1}' width='{width}' height='{height}'/></clipPath></defs>\
        <polygon points='{area}' fill='{BLUE}' fill-opacity='.35' clip-path='url(#above)'/><polygon points='{area}' fill='{RED}' fill-opacity='.35' clip-path='url(#below)'/>\
        <polyline points='{path}' fill='none' stroke='{GOLD}' stroke-width='2.5' stroke-linejoin='round'/>");
    cv.svg.push_str("</g></svg>");
    rasterize(&cv.svg, width, height)
}

/// Runs CPU-bound SVG rasterisation off the async runtime.
pub async fn blocking(work: impl FnOnce() -> Result<Vec<u8>> + Send + 'static) -> Result<Vec<u8>> {
    tokio::task::spawn_blocking(work).await.map_err(|_| anyhow::anyhow!("Image worker failed"))?
}

/// Page gradient, tracked-row gradients, header rules and the row shadow.
const SCOREBOARD_DEFS: &str = "<defs>\
<linearGradient id='page' x2='0' y2='1'><stop stop-color='#0e1628'/><stop offset='1' stop-color='#080d18'/></linearGradient>\
<linearGradient id='row100'><stop stop-color='#1f3c60'/><stop offset='.55' stop-color='#152a43'/><stop offset='1' stop-color='#111c2e'/></linearGradient>\
<linearGradient id='row200'><stop stop-color='#47263e'/><stop offset='.55' stop-color='#2c1f30'/><stop offset='1' stop-color='#181725'/></linearGradient>\
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
        Badge::Ace => (lang.t(Msg::ImgAce), "#c4cee4", "#2a3242"),
        Badge::Worst => (lang.t(Msg::ImgWorst), RED, "#3d1d26"),
    }
}
fn draw_team(cv: &mut Canvas, game: &Match, team: u16, players: &[&Participant], top: u32, lang: Lang, art: &Assets, badges: &[(u64, Badge)]) {
    let tint = color(team);
    let style = if team == 100 { "100" } else { "200" };
    let won = players.first().is_some_and(|p| p.win);
    let total_kills: i64 = players.iter().map(|p| p.kills).sum();
    let total_damage: i64 = players.iter().map(|p| p.damage).sum();
    let total_gold: i64 = players.iter().map(|p| p.gold).sum();
    // Open header: team, result and objectives as chips, totals on the right, a fading team rule.
    let name = team_name(team, lang);
    cv.text(40, top + 34, 26, tint, 700, "start", &name);
    let mut x = 40 + text_width(&name, 26) + 20;
    let result = lang.t(if game.remake { Msg::ImgRemake } else if won { Msg::ImgWin } else { Msg::ImgLoss });
    x += pill(cv, x, top + 12, 28, if won { "#1c4a42" } else { "#4d2635" }, if won { GREEN } else { RED }, 12, result) + 16;
    let objectives = game.objectives.get(&team.to_string());
    for (key, label) in [("dragon", Msg::ImgDragon), ("baron", Msg::ImgBaron), ("tower", Msg::ImgTower), ("inhibitor", Msg::ImgInhibitor)] {
        if let Some(count) = objectives.and_then(|o| o.get(key)) {
            x += pill(cv, x, top + 12, 28, "#172235", if *count > 0 { TEXT } else { FAINT }, 12, &format!("{}  {count}", lang.t(label))) + 10;
        }
    }
    cv.text(1560, top + 33, 16, DIM, 700, "end", &lang.f(Msg::ImgTeamTotals, &[&total_kills, &compact(total_gold), &compact(total_damage)]));
    let _ = write!(cv.svg, "<rect x='40' y='{}' width='1520' height='2' fill='url(#rule{style})'/>", top + 50);
    for (x, label) in [(512, "KDA"), (632, "CS"), (756, lang.t(Msg::ImgDamage)), (900, lang.t(Msg::ImgTaken)), (1030, lang.t(Msg::ImgGold)), (1138, lang.t(Msg::ImgVision)), (1350, lang.t(Msg::ImgItems))] {
        cv.text(x, top + 76, 11, FAINT, 700, "middle", label);
    }
    let best_damage = players.iter().map(|p| p.damage).max().unwrap_or(0);
    let best_cs = players.iter().map(|p| p.cs).max().unwrap_or(0);
    let best_gold = players.iter().map(|p| p.gold).max().unwrap_or(0);
    let minutes = (game.duration as f64 / 60.0).max(1.0);
    for (index, p) in players.iter().enumerate() {
        let y = top + 88 + index as u32 * ROW;
        let badge = badges.iter().find(|(id, _)| *id == p.id).map(|(_, b)| badge_style(*b, lang));
        // Tracked players stand out; everyone else sits on a quiet flat row.
        if p.tracked {
            let _ = write!(cv.svg, "<rect x='32' y='{y}' width='1536' height='72' rx='14' fill='url(#row{style})' filter='url(#shadow)'/>");
            cv.outline(32, y, 1536, 72, 14, if team == 100 { "rgba(114,184,250,.45)" } else { "rgba(250,130,151,.45)" });
            cv.rect(32, y + 14, 4, 44, 2, tint);
        } else {
            cv.rect(32, y, 1536, 72, 14, "#111a28");
        }
        cv.image(art.image(&format!("champion:{}", p.champion)), 49, y + 9, 54, 54, 12);
        cv.outline(49, y + 9, 54, 54, 12, badge.map_or(if p.tracked { tint } else { "#2c3a50" }, |(_, ink, _)| ink));
        if let Some((_, ink, _)) = badge { cv.outline(47, y + 7, 58, 58, 14, ink); }
        if p.level > 0 {
            cv.rect(80, y + 49, 27, 19, 9, "#0b1220");
            cv.text(94, y + 63, 11, TEXT, 700, "middle", &p.level.to_string());
        }
        for (i, id) in p.spells.iter().take(2).enumerate() { cv.image(art.image(&format!("spell:{id}")), 116, y + 10 + i as u32 * 28, 24, 24, 6); }
        for (i, id) in p.runes.iter().take(2).enumerate() { cv.image(art.image(&format!("rune:{id}")), 147 + i as u32 * 3, y + 10 + i as u32 * 31, if i == 0 { 25 } else { 19 }, if i == 0 { 25 } else { 19 }, 10); }
        cv.text(187, y + 29, 19, if p.tracked { TEXT } else { "#c3cbd8" }, if p.tracked { 700 } else { 400 }, "start", &short(&p.name, 22));
        let mut x = 187;
        if let Some((label, ink, fill)) = badge { x += pill(cv, x, y + 41, 19, fill, ink, 10, label) + 8; }
        let champion = art.names.get(&p.champion).map(String::as_str).unwrap_or(&p.champion);
        let secondary = match (&p.rank_after, lang.role(&p.role)) {
            (Some(rank), _) if p.tracked => lang.rank(rank),
            (_, "") => champion.to_owned(),
            (_, role) => format!("{champion} · {role}"),
        };
        let secondary = short(&secondary, 26);
        cv.text(x, y + 55, 12, DIM, 400, "start", &secondary);
        if let (true, Some(delta)) = (p.tracked, p.lp_delta) {
            let (fill, ink) = if delta > 0 { ("#183f37", GREEN) } else if delta < 0 { ("#4a2534", RED) } else { ("#263343", DIM) };
            pill(cv, x + text_width(&secondary, 12) + 8, y + 41, 19, fill, ink, 10, &format!("{delta:+} LP"));
        }
        for (x, value, c) in [(467, p.kills, TEXT), (512, p.deaths, RED), (557, p.assists, BLUE)] { cv.text(x, y + 30, 20, c, 700, "middle", &value.to_string()); }
        cv.text(490, y + 30, 17, FAINT, 400, "middle", "/");
        cv.text(535, y + 30, 17, FAINT, 400, "middle", "/");
        let ratio = (p.kills + p.assists) as f64 / p.deaths.max(1) as f64;
        let (ratio_fill, ratio_color) = if ratio >= 4.0 { ("#183f37", GREEN) } else if ratio < 1.0 { ("#4a2534", RED) } else { ("#222e40", DIM) };
        cv.rect(450, y + 42, 47, 19, 9, ratio_fill);
        cv.text(474, y + 56, 11, ratio_color, 700, "middle", &format!("{ratio:.1}"));
        let kp = ((p.kills + p.assists) as f64 / total_kills.max(1) as f64 * 100.0).clamp(0.0, 100.0);
        cv.text(506, y + 56, 11, FAINT, 400, "start", &format!("{kp:.0}% KP"));
        cv.text(632, y + 30, 20, if p.cs == best_cs { GOLD } else { TEXT }, 700, "middle", &p.cs.to_string());
        cv.text(632, y + 56, 11, FAINT, 400, "middle", &format!("{:.1}/m", p.cs as f64 / minutes));
        cv.text(756, y + 30, 20, if p.damage == best_damage { GOLD } else { TEXT }, 700, "middle", &number(p.damage));
        cv.text(756, y + 52, 11, FAINT, 400, "middle", &format!("{:.0}% · {:.0}/m", p.damage as f64 / total_damage.max(1) as f64 * 100.0, p.damage as f64 / minutes));
        cv.rect(704, y + 60, 104, 4, 2, "#2a3549");
        cv.rect(704, y + 60, (p.damage as f64 / best_damage.max(1) as f64 * 104.0).clamp(2.0, 104.0) as u32, 4, 2, if p.damage == best_damage { GOLD } else { tint });
        let taken = if p.damage_taken > 0 { number(p.damage_taken) } else { "—".into() };
        cv.text(900, y + 30, 19, "#d5dbe6", 400, "middle", &taken);
        cv.text(900, y + 56, 11, FAINT, 400, "middle", &format!("{:.0}/m", p.damage_taken as f64 / minutes));
        cv.text(1030, y + 30, 20, if p.gold == best_gold { GOLD } else { TEXT }, 700, "middle", &compact(p.gold));
        cv.text(1030, y + 56, 11, FAINT, 400, "middle", &format!("{:.0}/m", p.gold as f64 / minutes));
        cv.text(1138, y + 30, 19, "#d5dbe6", 400, "middle", &p.vision.to_string());
        cv.text(1138, y + 56, 11, FAINT, 400, "middle", &lang.f(Msg::ImgWards, &[&p.wards, &p.control_wards]));
        for (slot, item) in p.items.iter().take(7).enumerate() {
            let x = 1224 + slot as u32 * 40;
            let radius = if slot == 6 { 16 } else { 7 };
            if *item > 0 { cv.image(art.image(&format!("item:{item}")), x, y + 20, 33, 33, radius); } else { cv.rect(x, y + 20, 33, 33, radius, "#0d1422"); }
            cv.outline(x, y + 20, 33, 33, radius, "#2a3446");
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
