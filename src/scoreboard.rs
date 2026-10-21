//! The post-game scoreboard: a splash-art header with the result, then one gold-framed table per team.
use crate::{i18n::{Lang, Msg}, model::{Match, Participant, Rank}, presentation::{self, Badge}, render::{Canvas, badge_style, number, rasterize, short, team_name, text_width, tier_color},
    render_assets::Assets, theme::{BG, CREAM, GOLD, GOLD_DIM, PANEL, PANEL_DARK, ROW_A, ROW_B, RULE, SOFT, TEAL, team_color, team_ink, thousands}};
use anyhow::{Result, ensure};
use std::fmt::Write;

const W: u32 = 1200;
const HERO: f32 = 196.0;
const TEAM_H: f32 = 42.0;
const SUB_H: f32 = 20.0;
const ROW: f32 = 54.0;
// Column anchors: KDA and the small stat columns are centred, damage and taken start at their bar.
const SPELL_X: f32 = 90.0;
const RUNE_X: f32 = 114.0;
const NAME_X: f32 = 146.0;
const LP_X: f32 = 308.0;
const KDA_X: f32 = 456.0;
const DEALT_X: f32 = 540.0;
const TAKEN_X: f32 = 664.0;
const BAR_W: f32 = 100.0;
const CS_X: f32 = 800.0;
const GOLD_X: f32 = 858.0;
const VISION_X: f32 = 914.0;
const ITEMS_X: f32 = 952.0;
const TRACKED_NAME: &str = "#f0e6d2";
const OTHER_NAME: &str = "#cdbe91";
const TOP_GOLD: &str = "#f0c060";
const KDA_HIGH: &str = "#ff9f43";
const DEATH: &str = "#ff7b8f";
/// Objective icon name and the Match-V5 key whose count it shows.
const OBJECTIVES: [(&str, &str); 5] = [("dragon", "dragon"), ("baron", "baron"), ("herald", "riftHerald"), ("tower", "tower"), ("inhibitor", "inhibitor")];

type Team<'a> = (u16, Vec<&'a Participant>);

/// Everything a row needs to compare itself with the rest of the game.
struct Context<'a> { name_x: f32, art: &'a Assets, lang: Lang, badges: Vec<(u64, Badge)>, minutes: f64, max_dealt: i64, max_taken: i64, max_gold: i64 }

pub fn scoreboard(game: &Match, lang: Lang, timezone: &str, preview: bool, art: &Assets) -> Result<Vec<u8>> {
    ensure!(!game.participants.is_empty() && game.participants.len() <= 32, "Unsupported scoreboard size");
    let mut teams: Vec<Team> = Vec::new();
    for p in &game.participants {
        match teams.iter_mut().find(|(id, _)| *id == p.team) { Some((_, rows)) => rows.push(p), None => teams.push((p.team, vec![p])) }
    }
    teams.sort_by_key(|(id, rows)| (!rows.iter().any(|p| p.tracked), *id));
    for (_, rows) in &mut teams { rows.sort_by_key(|p| p.id); }
    let height = HERO + teams.iter().map(|(_, rows)| TEAM_H + SUB_H + ROW * rows.len() as f32 + 6.0).sum::<f32>() + 8.0;
    ensure!(height <= 5000.0, "Scoreboard exceeds image bounds");
    let mut cv = Canvas { clips: 0, svg: format!("<svg xmlns='http://www.w3.org/2000/svg' width='{W}' height='{height}'><g font-family='Be Vietnam Pro,DejaVu Sans,sans-serif'>") };
    let backdrop = cv.gradient(&[(0.0, "#0a1a28", 1.0), (0.5, BG, 1.0), (1.0, "#0a1420", 1.0)], (0.0, 0.0, 0.0, 1.0));
    cv.block(0.0, 0.0, W as f32, height, 0.0, &backdrop, "");
    header(&mut cv, game, &teams, lang, timezone, preview, art)?;
    // Matches stored before spells and runes were captured leave that column empty; pull the names left.
    let name_x = if game.participants.iter().all(|p| p.spells.is_empty() && p.runes.is_empty()) { 90.0 } else { NAME_X };
    let ctx = Context {
        name_x, art, lang, badges: presentation::badges(game), minutes: (game.duration as f64 / 60.0).max(1.0),
        max_dealt: game.participants.iter().map(|p| p.damage).max().unwrap_or(1).max(1),
        max_taken: game.participants.iter().map(|p| p.damage_taken).max().unwrap_or(1).max(1),
        max_gold: game.participants.iter().map(|p| p.gold).max().unwrap_or(1).max(1),
    };
    let mut y = HERO;
    for (team, rows) in &teams {
        team_header(&mut cv, game, *team, rows, y, &ctx);
        y += TEAM_H;
        column_labels(&mut cv, y, lang);
        y += SUB_H;
        let team_kills: i64 = rows.iter().map(|p| p.kills).sum();
        for (i, p) in rows.iter().enumerate() {
            row(&mut cv, p, y, i, team_kills, &ctx);
            y += ROW;
        }
        y += 6.0;
    }
    cv.stroke(1.0, 1.0, W as f32 - 2.0, height - 2.0, 0.0, GOLD_DIM, 2.0, 1.0);
    cv.svg.push_str("</g></svg>");
    rasterize(&cv.svg, W, height.ceil() as u32)
}

/// Splash art of the featured champion with the result, date, gold lead and kill score over it.
fn header(cv: &mut Canvas, game: &Match, teams: &[Team], lang: Lang, timezone: &str, preview: bool, art: &Assets) -> Result<()> {
    let tracked: Vec<_> = game.participants.iter().filter(|p| p.tracked).collect();
    let split = tracked.iter().any(|p| tracked[0].team != p.team);
    let won = teams.first().is_some_and(|(_, rows)| rows[0].win);
    let word = lang.t(if game.remake { Msg::ImgRemake } else if split { Msg::ImgResult } else if won { Msg::ImgVictory } else { Msg::ImgDefeat });
    let tz = timezone.parse::<chrono_tz::Tz>().map_err(|_| anyhow::anyhow!("Invalid image timezone"))?;
    let ended = chrono::DateTime::from_timestamp(game.ended_at, 0).ok_or_else(|| anyhow::anyhow!("Invalid image timestamp"))?.with_timezone(&tz);
    let backdrop = cv.gradient(&[(0.0, "#0f2a3a", 1.0), (1.0, "#142b3a", 1.0)], (0.0, 0.0, 1.0, 1.0));
    cv.block(0.0, 0.0, W as f32, HERO, 0.0, &backdrop, "");
    let splash = presentation::featured(game).and_then(|p| art.image(&format!("splash:{}", p.champion)));
    if let Some(data) = splash {
        let _ = write!(cv.svg, "<defs><clipPath id='hero'><rect width='{W}' height='{HERO}'/></clipPath></defs><image href='{data}' x='0' y='-120' width='{W}' height='708' preserveAspectRatio='xMidYMin slice' clip-path='url(#hero)'/>");
    }
    let sides = cv.gradient(&[(0.0, BG, 0.92), (0.3, BG, 0.62), (0.48, BG, 0.0), (0.8, BG, 0.0), (1.0, BG, 0.72)], (0.0, 0.0, 1.0, 0.0));
    cv.block(0.0, 0.0, W as f32, HERO, 0.0, &sides, "");
    let floor = cv.gradient(&[(0.0, BG, 0.0), (0.7, BG, 0.0), (1.0, BG, 1.0)], (0.0, 0.0, 0.0, 1.0));
    cv.block(0.0, 0.0, W as f32, HERO, 0.0, &floor, "");
    // Queue line with a gold diamond, then the result word in gold (red for a defeat) with a soft shadow.
    let _ = write!(cv.svg, "<polygon points='43,22 50,29 43,36 36,29' fill='none' stroke='{GOLD}' stroke-width='1.5'/>");
    cv.label(62.0, 34.0, 12.0, GOLD, 700, "start", 5.0, &lang.queue_title(game.queue).to_uppercase());
    let ink = if !game.remake && !split && !won { cv.gradient(&[(0.0, "#ffb3bd", 1.0), (0.5, "#e84057", 1.0), (1.0, "#8c2232", 1.0)], (0.0, 0.0, 0.0, 1.0)) }
        else { cv.gradient(&[(0.0, "#f9e7b0", 1.0), (0.5, GOLD, 1.0), (1.0, "#8c6a2e", 1.0)], (0.0, 0.0, 0.0, 1.0)) };
    let _ = write!(cv.svg, "<text x='34' y='104' font-size='60' font-weight='700' fill='#000' fill-opacity='.45' letter-spacing='3' transform='translate(2 3)'>{}</text>", crate::render::escape(word));
    cv.label(34.0, 104.0, 60.0, &ink, 700, "start", 3.0, word);
    let sub = format!("{}:{:02}  ·  {}", game.duration / 60, game.duration % 60, ended.format(lang.t(Msg::ImgDateFormat)));
    cv.label(36.0, 132.0, 14.0, "#e4dcc8", 400, "start", 1.5, &sub);
    if preview {
        let text = lang.t(Msg::ImgPreview);
        let w = text_width(text, 10) as f32 + 24.0;
        cv.block(1168.0 - w, 4.0, w, 16.0, 0.0, BG, "fill-opacity='.8'");
        cv.label(1168.0 - w / 2.0, 15.5, 10.0, SOFT, 700, "middle", 1.0, text);
    }
    if teams.len() < 2 { cv.hairline(0.0, W as f32, HERO - 1.0, 0.9); return Ok(()); }
    // Gold lead of the first (tracked) team over the second, as one proportional bar.
    let gold = |rows: &[&Participant]| rows.iter().map(|p| p.gold).sum::<i64>();
    let ((id1, a), (id2, b)) = ((teams[0].0, gold(&teams[0].1)), (teams[1].0, gold(&teams[1].1)));
    let (x, w, y) = (36.0f32, 380.0f32, 150.0f32);
    let share = (w * a as f32 / (a + b).max(1) as f32).round();
    cv.block(x - 2.0, y - 2.0, w + 4.0, 12.0, 0.0, BG, &format!("fill-opacity='.7' stroke='{GOLD}' stroke-width='1'"));
    let left = cv.gradient(&[(0.0, team_color(id1), 0.75), (1.0, team_color(id1), 1.0)], (0.0, 0.0, 1.0, 0.0));
    let right = cv.gradient(&[(0.0, team_color(id2), 0.75), (1.0, team_color(id2), 1.0)], (0.0, 0.0, 1.0, 0.0));
    cv.block(x, y, share, 8.0, 0.0, &left, "");
    cv.block(x + share, y, w - share, 8.0, 0.0, &right, "");
    cv.block(x + share - 1.0, y - 5.0, 2.0, 18.0, 0.0, CREAM, "");
    cv.label(x, y + 28.0, 13.0, team_ink(id1), 700, "start", 0.0, &thousands(a));
    cv.label(x + w, y + 28.0, 13.0, team_ink(id2), 700, "end", 0.0, &thousands(b));
    cv.label(x + w / 2.0, y + 28.0, 11.0, GOLD, 700, "middle", 2.0, &format!("{}  {:+.1}k", lang.t(Msg::ImgGold), (a - b) as f64 / 1000.0));
    // Kill score, framed in gold.
    cv.block(1000.0, 26.0, 168.0, 120.0, 0.0, BG, &format!("fill-opacity='.72' stroke='{GOLD}' stroke-width='1.2'"));
    cv.stroke(1006.0, 32.0, 156.0, 108.0, 0.0, GOLD_DIM, 0.8, 1.0);
    cv.line(1084.0, 48.0, 1084.0, 130.0, GOLD_DIM, 1.0);
    for (cx, (id, rows)) in [(1042.0, &teams[0]), (1126.0, &teams[1])] {
        cv.label(cx, 56.0, 9.0, team_ink(*id), 700, "middle", 1.5, &team_name(*id, lang));
        cv.label(cx, 112.0, 46.0, CREAM, 700, "middle", 0.0, &rows.iter().map(|p| p.kills).sum::<i64>().to_string());
    }
    cv.hairline(0.0, W as f32, HERO - 1.0, 0.9);
    Ok(())
}

fn team_header(cv: &mut Canvas, game: &Match, team: u16, rows: &[&Participant], y: f32, ctx: &Context) {
    let lang = ctx.lang;
    let won = rows[0].win;
    cv.block(0.0, y, W as f32, TEAM_H, 0.0, PANEL, "");
    cv.hairline(0.0, W as f32, y, 0.6);
    cv.block(0.0, y, 4.0, TEAM_H, 0.0, team_color(team), "");
    let name = team_name(team, lang);
    cv.label(32.0, y + 27.0, 16.0, team_ink(team), 700, "start", 3.0, &name);
    let chip_x = 32.0 + name.chars().count() as f32 * 12.6 + 14.0;
    let result = lang.t(if game.remake { Msg::ImgRemake } else if won { Msg::ImgWin } else { Msg::ImgLoss });
    let chip = (text_width(result, 10) as f32 + 28.0).max(68.0);
    cv.block(chip_x, y + 11.0, chip, 20.0, 0.0, BG, &format!("stroke='{}'", if won { GOLD } else { "#5a5f68" }));
    cv.label(chip_x + chip / 2.0, y + 25.5, 10.5, if won { GOLD } else { "#8a8f98" }, 700, "middle", 2.0, result);
    // Objectives this team took (only the kinds the game reports).
    let count = |key: &str| game.objectives.get(&team.to_string()).and_then(|o| o.get(key)).copied();
    let kinds: Vec<_> = OBJECTIVES.iter().filter(|(_, key)| count(key).is_some()).collect();
    if !kinds.is_empty() {
        let divider = chip_x + chip + 22.0;
        cv.line(divider, y + 10.0, divider, y + TEAM_H - 10.0, GOLD_DIM, 1.0);
        for (k, (icon, key)) in kinds.iter().enumerate() {
            let x = divider + 18.0 + k as f32 * 62.0;
            cv.picture(ctx.art.image(&format!("objective:{icon}-{team}")), x, y + 10.0, 24.0, 24.0, 0.0, "xMidYMid meet");
            cv.label(x + 30.0, y + 28.0, 14.0, CREAM, 700, "start", 0.0, &count(key).unwrap_or(0).to_string());
        }
    }
    if let Some(rank) = average_rank(rows) {
        cv.label(886.0, y + 28.0, 9.5, SOFT, 700, "end", 1.5, lang.t(Msg::ImgAvgRank));
        rank_emblem(cv, ctx.art, 894.0, y + 12.0, 30.0, 19.0, &rank.tier);
        cv.label(930.0, y + 28.0, 14.0, tier_color(&rank.tier), 700, "start", 0.0, &lang.rank_name(&rank));
    }
    let _ = write!(cv.svg, "<text x='1168' y='{}' font-size='16' font-weight='700' fill='{CREAM}' text-anchor='end'><tspan font-size='9.5' fill='{SOFT}' letter-spacing='1.5'>{}  </tspan>{}</text>",
        y + 28.0, lang.t(Msg::ImgDamage), thousands(rows.iter().map(|p| p.damage).sum()));
    cv.line(0.0, y + TEAM_H, W as f32, y + TEAM_H, GOLD, 1.0);
}

fn column_labels(cv: &mut Canvas, y: f32, lang: Lang) {
    cv.block(0.0, y, W as f32, SUB_H, 0.0, PANEL_DARK, "");
    for (x, anchor, label) in [(KDA_X, "middle", "KDA"), (DEALT_X, "start", lang.t(Msg::ImgDamage)), (TAKEN_X, "start", lang.t(Msg::ImgTaken)), (CS_X, "middle", "CS"),
        (GOLD_X, "middle", lang.t(Msg::ImgGold)), (VISION_X, "middle", lang.t(Msg::ImgVision)), (ITEMS_X + 106.0, "middle", lang.t(Msg::ImgItems))] {
        cv.label(x, y + 14.0, 9.5, SOFT, 700, anchor, 1.0, label);
    }
}

fn row(cv: &mut Canvas, p: &Participant, y: f32, index: usize, team_kills: i64, ctx: &Context) {
    let (art, lang) = (ctx.art, ctx.lang);
    cv.block(0.0, y, W as f32, ROW, 0.0, if index.is_multiple_of(2) { ROW_A } else { ROW_B }, "");
    if p.tracked {
        let wash = cv.gradient(&[(0.0, TEAL, 0.20), (0.55, TEAL, 0.05), (1.0, TEAL, 0.0)], (0.0, 0.0, 1.0, 0.0));
        cv.block(0.0, y, W as f32, ROW, 0.0, &wash, "");
        cv.block(0.0, y, 4.0, ROW, 0.0, GOLD, "");
    }
    cv.line(0.0, y + ROW, W as f32, y + ROW, RULE, 1.0);
    // Portrait with its level, and the MVP / ACE / worst label across the top edge.
    let badge = ctx.badges.iter().find(|(id, _)| *id == p.id).map(|(_, b)| badge_style(*b, lang));
    cv.picture(art.image(&format!("champion:{}", p.champion)), 32.0, y + 6.0, 42.0, 42.0, 0.0, "xMidYMid slice");
    cv.frame(32.0, y + 6.0, 42.0, 42.0, badge.map_or(GOLD, |(_, ink, _)| ink));
    if p.level > 0 {
        let _ = write!(cv.svg, "<circle cx='71' cy='{}' r='8' fill='{BG}' stroke='{GOLD}' stroke-width='1.1'/>", y + 46.0);
        cv.label(71.0, y + 49.5, 9.0, CREAM, 700, "middle", 0.0, &p.level.to_string());
    }
    if let Some((label, ink, _)) = badge {
        let w = text_width(label, 9) as f32 + 14.0;
        cv.block(53.0 - w / 2.0, y - 2.0, w, 13.0, 0.0, BG, &format!("stroke='{ink}'"));
        cv.label(53.0, y + 8.0, 9.0, ink, 700, "middle", 1.0, label);
    }
    // Summoner spells and the keystone / secondary rune tree.
    for (k, id) in p.spells.iter().take(2).enumerate() { cv.picture(art.image(&format!("spell:{id}")), SPELL_X, y + 6.0 + k as f32 * 23.0, 20.0, 20.0, 2.0, "xMidYMid slice"); }
    if let Some(id) = p.runes.first() { cv.picture(art.image(&format!("rune:{id}")), RUNE_X, y + 6.0, 22.0, 22.0, 11.0, "xMidYMid slice"); }
    if let Some(id) = p.runes.get(1) { cv.picture(art.image(&format!("rune:{id}")), RUNE_X + 3.0, y + 32.0, 16.0, 16.0, 8.0, "xMidYMid slice"); }
    cv.label(ctx.name_x, y + 23.0, 15.0, if p.tracked { TRACKED_NAME } else { OTHER_NAME }, if p.tracked { 700 } else { 400 }, "start", 0.0, &short(&p.name, 20));
    identity(cv, p, y, ctx);
    // KDA with the participation line under it.
    let kda = (p.kills + p.assists) as f64 / p.deaths.max(1) as f64;
    cv.runs(KDA_X, y + 24.0, 18.0, "middle", &[(&p.kills.to_string(), CREAM, 700), (" / ", GOLD_DIM, 400), (&p.deaths.to_string(), DEATH, 700), (" / ", GOLD_DIM, 400), (&p.assists.to_string(), CREAM, 700)]);
    let participation = ((p.kills + p.assists) as f64 / team_kills.max(1) as f64 * 100.0).clamp(0.0, 100.0);
    let (text, ink) = if p.deaths == 0 { (lang.t(Msg::ImgPerfectKda).to_owned(), KDA_HIGH) } else { (format!("{kda:.1} KDA"), if kda >= 3.0 { KDA_HIGH } else { SOFT }) };
    cv.runs(KDA_X, y + 42.0, 11.5, "middle", &[(&text, ink, 700), (&format!("  ·  {participation:.0}% KP"), SOFT, 400)]);
    for (x, value, max, fill) in [(DEALT_X, p.damage, ctx.max_dealt, team_color(p.team)), (TAKEN_X, p.damage_taken, ctx.max_taken, "#7d8795")] {
        cv.label(x, y + 23.0, 13.5, CREAM, 400, "start", 0.0, &number(value));
        cv.block(x, y + 35.0, BAR_W, 6.0, 0.0, BG, "stroke='#3a3326' stroke-width='.8'");
        cv.block(x, y + 35.0, (value as f32 / max as f32 * BAR_W).clamp(3.0, BAR_W), 6.0, 0.0, fill, "");
    }
    let minutes = ctx.minutes;
    for (x, top, ink, sub) in [(CS_X, p.cs.to_string(), CREAM, format!("{:.1}/m", p.cs as f64 / minutes)),
        (GOLD_X, thousands(p.gold), if p.gold == ctx.max_gold { TOP_GOLD } else { CREAM }, format!("{:.0}/m", p.gold as f64 / minutes)),
        (VISION_X, p.vision.to_string(), CREAM, format!("{:.1}/m", p.vision as f64 / minutes))] {
        cv.label(x, y + 23.0, 14.0, ink, 700, "middle", 0.0, &top);
        cv.label(x, y + 41.0, 11.0, SOFT, 400, "middle", 0.0, &sub);
    }
    for slot in 0..7 {
        let (x, radius) = (ITEMS_X + slot as f32 * 31.0, if slot == 6 { 14.0 } else { 0.0 });
        match p.items.get(slot).filter(|item| **item > 0) {
            Some(item) => cv.picture(art.image(&format!("item:{item}")), x, y + 13.0, 28.0, 28.0, radius, "xMidYMid slice"),
            None => cv.block(x, y + 13.0, 28.0, 28.0, radius, BG, ""),
        }
        cv.stroke(x, y + 13.0, 28.0, 28.0, radius, GOLD_DIM, 1.0, 1.0);
    }
}

/// Rank with its emblem (and the LP change of tracked players); champion and role when the rank is unknown.
fn identity(cv: &mut Canvas, p: &Participant, y: f32, ctx: &Context) {
    let (art, lang, name_x) = (ctx.art, ctx.lang, ctx.name_x);
    let Some(rank) = &p.rank_after else {
        let champion = art.names.get(&p.champion).map(String::as_str).unwrap_or(&p.champion);
        let text = match lang.role(&p.role) { "" => champion.to_owned(), role => format!("{champion} · {role}") };
        cv.label(name_x, y + 42.0, 12.0, SOFT, 400, "start", 0.0, &short(&text, 28));
        return;
    };
    rank_emblem(cv, art, name_x - 2.0, y + 28.0, 28.0, 17.0, &rank.tier);
    cv.runs(name_x + 30.0, y + 42.0, 12.0, "start", &[(&lang.rank_name(rank), tier_color(&rank.tier), 700), ("  ·  ", "#6e6a5f", 400), (&format!("{} LP", rank.lp), "#d6cfbd", 400)]);
    if let (true, Some(delta)) = (p.tracked, p.lp_delta) { cv.delta_pill(LP_X, y + 28.0, delta, &format!("{} LP", delta.abs())); }
}

/// The tier's emblem fitted into a box; a plain diamond in the tier colour when the artwork is unavailable.
fn rank_emblem(cv: &mut Canvas, art: &Assets, x: f32, y: f32, w: f32, h: f32, tier: &str) {
    match art.image(&format!("tier:{tier}")) {
        Some(data) => cv.picture(Some(data), x, y, w, h, 0.0, "xMidYMid meet"),
        None => { let (cx, cy) = (x + w / 2.0, y + h / 2.0); cv.polygon(&format!("{},{} {},{} {},{} {},{}", cx, cy - 8.0, cx + 8.0, cy, cx, cy + 8.0, cx - 8.0, cy), tier_color(tier)); }
    }
}

/// Mean of the known ranks as a tier and division, for the team header.
fn average_rank(rows: &[&Participant]) -> Option<Rank> {
    let values: Vec<i64> = rows.iter().filter_map(|p| p.rank_after.as_ref()?.absolute()).collect();
    if values.is_empty() { return None; }
    let mean = values.iter().sum::<i64>() / values.len() as i64;
    let tiers = [("IRON", 0), ("BRONZE", 400), ("SILVER", 800), ("GOLD", 1200), ("PLATINUM", 1600), ("EMERALD", 2000), ("DIAMOND", 2400)];
    let (tier, base) = if mean >= 2800 { ("MASTER", 2800) } else { tiers.iter().rev().find(|(_, base)| mean >= *base).copied().unwrap_or(("IRON", 0)) };
    let division = if base == 2800 { "" } else { ["IV", "III", "II", "I"][((mean - base) / 100).clamp(0, 3) as usize] };
    Some(Rank { queue: 0, tier: tier.into(), division: division.into(), lp: 0 })
}
