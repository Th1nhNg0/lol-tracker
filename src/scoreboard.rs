//! The post-game scoreboard: tinted team blocks and, between them, objectives with kill and gold bars.
use crate::{i18n::{Lang, Msg}, model::{Match, Participant}, presentation::{self, Badge}, render::{Canvas, DIM, GREEN, RED, TEXT, badge_style, color, number, rasterize, short, team_name, text_width, tier_color}, render_assets::Assets};
use anyhow::{Result, ensure};
use std::fmt::Write;

const W: u32 = 1200;
const HEAD: u32 = 56;
const TEAM_H: u32 = 36;
const ROW: u32 = 58;
const SUMMARY_H: u32 = 84;
// Column anchors: KDA and the stat columns are centred, damage and taken start at their bar.
const SPELL_X: u32 = 68;
const NAME_X: u32 = 126;
const KDA_X: u32 = 414;
const DEALT_X: u32 = 494;
const TAKEN_X: u32 = 640;
const BAR_W: u32 = 128;
const VISION_X: u32 = 830;
const CS_X: u32 = 898;
const ITEMS_X: u32 = 946;
const BLUE_BAR: &str = "#3d7cff";
const RED_BAR: &str = "#e84057";
const MUTED: &str = "#9aa6c0";
/// Objective icon name and the Match-V5 key whose count it shows.
const OBJECTIVES: [(&str, &str); 5] = [("dragon", "dragon"), ("baron", "baron"), ("herald", "riftHerald"), ("tower", "tower"), ("inhibitor", "inhibitor")];

/// Row and header fills of a team block, tinted by the team's result.
pub(crate) struct Block { pub header: &'static str, pub title: &'static str, pub rows: [&'static str; 2], pub tracked: &'static str }
pub(crate) const VICTORY: Block = Block { header: "#16213d", title: "#6fa0ff", rows: ["#1d2c4f", "#223259"], tracked: "#2f4a86" };
pub(crate) const DEFEAT: Block = Block { header: "#2b1824", title: "#ff7a8c", rows: ["#3a2030", "#43263a"], tracked: "#5d2f48" };

pub fn scoreboard(game: &Match, lang: Lang, timezone: &str, preview: bool, art: &Assets) -> Result<Vec<u8>> {
    ensure!(!game.participants.is_empty() && game.participants.len() <= 32, "Unsupported scoreboard size");
    let mut teams: Vec<(u16, Vec<&Participant>)> = Vec::new();
    for p in &game.participants {
        match teams.iter_mut().find(|(id, _)| *id == p.team) { Some((_, rows)) => rows.push(p), None => teams.push((p.team, vec![p])) }
    }
    teams.sort_by_key(|(id, rows)| (!rows.iter().any(|p| p.tracked), *id));
    for (_, rows) in &mut teams { rows.sort_by_key(|p| p.id); }
    let summary = teams.len() >= 2;
    let height = HEAD + teams.iter().map(|(_, rows)| TEAM_H + ROW * rows.len() as u32).sum::<u32>() + if summary { SUMMARY_H } else { 0 };
    ensure!(height <= 5000, "Scoreboard exceeds image bounds");
    let mut cv = Canvas { clips: 0, svg: format!("<svg xmlns='http://www.w3.org/2000/svg' width='{W}' height='{height}'><g font-family='Be Vietnam Pro,DejaVu Sans,sans-serif'>") };
    cv.rect(0, 0, W, height, 0, "#0b1020");
    let badges = presentation::badges(game);
    header(&mut cv, game, &teams, lang, timezone, preview)?;
    let max_dealt = game.participants.iter().map(|p| p.damage).max().unwrap_or(1).max(1);
    let max_taken = game.participants.iter().map(|p| p.damage_taken).max().unwrap_or(1).max(1);
    let minutes = (game.duration as f64 / 60.0).max(1.0);
    let mut y = HEAD;
    for (index, (team, rows)) in teams.iter().enumerate() {
        let won = rows[0].win;
        let block = if won { &VICTORY } else { &DEFEAT };
        let result = lang.t(if game.remake { Msg::ImgRemake } else if won { Msg::ImgWin } else { Msg::ImgLoss });
        cv.rect(0, y, W, TEAM_H, 0, block.header);
        cv.text(18, y + 24, 14, block.title, 700, "start", &format!("{result}  ·  {}", team_name(*team, lang)));
        for (x, label) in [(KDA_X, "KDA"), (DEALT_X + BAR_W / 2, lang.t(Msg::ImgDamage)), (TAKEN_X + BAR_W / 2, lang.t(Msg::ImgTaken)), (VISION_X, lang.t(Msg::ImgVision)), (CS_X, "CS"), (ITEMS_X + 110, lang.t(Msg::ImgItems))] {
            cv.text(x, y + 23, 10, "#8b97b3", 700, "middle", label);
        }
        for (i, p) in rows.iter().enumerate() {
            let ry = y + TEAM_H + i as u32 * ROW;
            cv.rect(0, ry, W, ROW, 0, if p.tracked { block.tracked } else { block.rows[i % 2] });
            portrait(&mut cv, p, ry + 6, art, &badges, lang);
            spell_rune(&mut cv, p, ry + 7, art);
            cv.text(NAME_X, ry + 24, 14, if p.tracked { TEXT } else { "#cfd6e4" }, if p.tracked { 700 } else { 400 }, "start", &short(&p.name, 22));
            identity(&mut cv, p, ry + 44, art, lang);
            let team_kills: i64 = rows.iter().map(|q| q.kills).sum();
            let participation = ((p.kills + p.assists) as f64 / team_kills.max(1) as f64 * 100.0).clamp(0.0, 100.0);
            cv.text(KDA_X, ry + 24, 13, TEXT, 400, "middle", &format!("{}/{}/{} ({participation:.0}%)", p.kills, p.deaths, p.assists));
            let ratio = (p.kills + p.assists) as f64 / p.deaths.max(1) as f64;
            let (text, ink) = if p.deaths == 0 { ("Perfect".to_owned(), "#ff9f43") } else { (format!("{ratio:.2}:1"), if ratio >= 3.0 { "#ff9f43" } else { "#b6c0d0" }) };
            cv.text(KDA_X, ry + 43, 13, ink, 700, "middle", &text);
            for (x, value, max, fill) in [(DEALT_X, p.damage, max_dealt, team_bar(p.team)), (TAKEN_X, p.damage_taken, max_taken, "#8591a8")] {
                cv.text(x, ry + 24, 13, TEXT, 400, "start", &number(value));
                cv.rect(x, ry + 32, BAR_W, 7, 0, "#1b2639");
                cv.rect(x, ry + 32, (value as f64 / max as f64 * f64::from(BAR_W)).clamp(2.0, f64::from(BAR_W)) as u32, 7, 0, fill);
            }
            cv.text(VISION_X, ry + 24, 14, TEXT, 700, "middle", &p.vision.to_string());
            cv.text(VISION_X, ry + 43, 12, MUTED, 400, "middle", &format!("{} / {}", p.wards, p.control_wards));
            cv.text(CS_X, ry + 24, 14, TEXT, 700, "middle", &p.cs.to_string());
            cv.text(CS_X, ry + 43, 12, MUTED, 400, "middle", &format!("{:.1}/m", p.cs as f64 / minutes));
            items(&mut cv, p, ry + 14, art);
        }
        y += TEAM_H + ROW * rows.len() as u32;
        if index == 0 && summary {
            draw_summary(&mut cv, game, &teams, y, lang, art);
            y += SUMMARY_H;
        }
    }
    cv.svg.push_str("</g></svg>");
    rasterize(&cv.svg, W, height)
}

fn team_bar(team: u16) -> &'static str { if team == 100 { BLUE_BAR } else { RED_BAR } }

fn header(cv: &mut Canvas, game: &Match, teams: &[(u16, Vec<&Participant>)], lang: Lang, timezone: &str, preview: bool) -> Result<()> {
    let tracked: Vec<_> = game.participants.iter().filter(|p| p.tracked).collect();
    let split = tracked.iter().any(|p| tracked[0].team != p.team);
    let won = teams.first().is_some_and(|(_, rows)| rows[0].win);
    let word = lang.t(if game.remake { Msg::ImgRemake } else if split { Msg::ImgResult } else if won { Msg::ImgVictory } else { Msg::ImgDefeat });
    let accent = if game.remake || split { DIM } else if won { GREEN } else { RED };
    let tz = timezone.parse::<chrono_tz::Tz>().map_err(|_| anyhow::anyhow!("Invalid image timezone"))?;
    let ended = chrono::DateTime::from_timestamp(game.ended_at, 0).ok_or_else(|| anyhow::anyhow!("Invalid image timestamp"))?.with_timezone(&tz);
    cv.rect(0, 0, W, HEAD, 0, "#0f1424");
    cv.text(18, 37, 22, accent, 700, "start", word);
    let sub = format!("{}  ·  {}:{:02}  ·  {}", lang.queue(game.queue), game.duration / 60, game.duration % 60, ended.format(lang.t(Msg::ImgDateFormat)));
    cv.text(18 + text_width(word, 22) + 18, 36, 13, "#c4cddb", 400, "start", &sub);
    if preview { cv.text(W - 190, 36, 10, DIM, 400, "end", lang.t(Msg::ImgPreview)); }
    if teams.len() >= 2 {
        for (side, (id, rows)) in teams.iter().take(2).enumerate() {
            let kills: i64 = rows.iter().map(|p| p.kills).sum();
            cv.text(if side == 0 { W - 128 } else { W - 90 }, 40, 30, color(*id), 700, if side == 0 { "end" } else { "start" }, &kills.to_string());
        }
        cv.text(W - 109, 36, 10, "#4a566b", 700, "middle", "–");
    }
    Ok(())
}

/// Between the teams: objectives on the outer sides, kills and gold as one proportional bar each.
fn draw_summary(cv: &mut Canvas, game: &Match, teams: &[(u16, Vec<&Participant>)], y: u32, lang: Lang, art: &Assets) {
    cv.rect(0, y, W, SUMMARY_H, 0, "#0f1424");
    let count = |team: u16, key: &str| game.objectives.get(&team.to_string()).and_then(|o| o.get(key)).copied();
    for (side, (id, _)) in teams.iter().take(2).enumerate() {
        let kinds: Vec<_> = OBJECTIVES.iter().filter(|(_, key)| count(*id, key).is_some()).collect();
        for (k, (icon, key)) in kinds.iter().enumerate() {
            let x = if side == 0 { 16 + k as u32 * 46 } else { W - 16 - (kinds.len() - k) as u32 * 46 };
            if let Some(data) = art.image(&format!("objective:{icon}-{id}")) {
                let _ = write!(cv.svg, "<image href='{data}' x='{x}' y='{}' width='24' height='24'/>", y + 30);
            }
            cv.text(x + 30, y + 48, 14, TEXT, 700, "start", &count(*id, key).unwrap_or(0).to_string());
        }
    }
    let (left, wide) = (290, 620);
    let sums = |f: fn(&Participant) -> i64| (teams[0].1.iter().map(|p| f(p)).sum::<i64>(), teams[1].1.iter().map(|p| f(p)).sum::<i64>());
    for (row, (label, (a, b))) in [(lang.t(Msg::ImgTotalKills), sums(|p| p.kills)), (lang.t(Msg::ImgTotalGold), sums(|p| p.gold))].into_iter().enumerate() {
        let by = y + 12 + row as u32 * 32;
        let share = (a as f64 / (a + b).max(1) as f64 * f64::from(wide)) as u32;
        cv.rect(left, by, share, 24, 0, team_bar(teams[0].0));
        cv.rect(left + share, by, wide - share, 24, 0, team_bar(teams[1].0));
        cv.text(left + 10, by + 17, 13, "#ffffff", 700, "start", &number(a));
        cv.text(left + wide - 10, by + 17, 13, "#ffffff", 700, "end", &number(b));
        cv.text(left + wide / 2, by + 17, 13, "#ffffff", 700, "middle", label);
    }
}

/// Champion portrait with its level, and the MVP / worst-player label across the top edge.
fn portrait(cv: &mut Canvas, p: &Participant, y: u32, art: &Assets, badges: &[(u64, Badge)], lang: Lang) {
    let badge = badges.iter().find(|(id, _)| *id == p.id).map(|(_, b)| badge_style(*b, lang));
    cv.image(art.image(&format!("champion:{}", p.champion)), 14, y, 46, 46, 2);
    cv.outline(14, y, 46, 46, 2, badge.map_or(if p.tracked { color(p.team) } else { "#2a374d" }, |(_, ink, _)| ink));
    if p.level > 0 {
        cv.rect(45, y + 33, 15, 13, 2, "#080c15");
        cv.text(52, y + 43, 9, TEXT, 700, "middle", &p.level.to_string());
    }
    if let Some((label, ink, fill)) = badge {
        let width = text_width(label, 9) + 8;
        cv.rect(37 - width / 2, y - 7, width, 14, 2, "#080c15");
        cv.rect(37 - width / 2, y - 7, width, 14, 2, fill);
        cv.text(37, y + 3, 9, ink, 700, "middle", label);
    }
}

/// Two summoner spells and the keystone and secondary rune tree.
fn spell_rune(cv: &mut Canvas, p: &Participant, y: u32, art: &Assets) {
    for (k, id) in p.spells.iter().take(2).enumerate() { cv.image(art.image(&format!("spell:{id}")), SPELL_X, y + k as u32 * 24, 22, 22, 3); }
    if let Some(id) = p.runes.first() { cv.image(art.image(&format!("rune:{id}")), SPELL_X + 26, y, 22, 22, 11); }
    if let Some(id) = p.runes.get(1) { cv.image(art.image(&format!("rune:{id}")), SPELL_X + 29, y + 27, 16, 16, 8); }
}

/// The tier's emblem as a 20 px icon; a plain diamond in the tier colour when the artwork is unavailable.
pub(crate) fn rank_icon(cv: &mut Canvas, art: &Assets, x: u32, base: u32, tier: &str) {
    match art.image(&format!("tier:{tier}")) {
        Some(data) => { let _ = write!(cv.svg, "<image href='{data}' x='{x}' y='{}' width='22' height='22'/>", base - 15); }
        None => { let _ = write!(cv.svg, "<polygon points='{},{} {},{} {},{} {},{}' fill='{}'/>", x + 11, base - 13, x + 18, base - 4, x + 11, base + 5, x + 4, base - 4, tier_color(tier)); }
    }
}

/// Rank with its emblem and a tier-coloured name (and the LP change for tracked players); champion and role when the rank is unknown.
fn identity(cv: &mut Canvas, p: &Participant, base: u32, art: &Assets, lang: Lang) {
    let mut x = NAME_X;
    let Some(rank) = &p.rank_after else {
        let champion = art.names.get(&p.champion).map(String::as_str).unwrap_or(&p.champion);
        let text = match lang.role(&p.role) { "" => champion.to_owned(), role => format!("{champion} · {role}") };
        cv.text(x, base, 11, DIM, 400, "start", &short(&text, 28));
        return;
    };
    rank_icon(cv, art, x, base, &rank.tier);
    x += 27;
    cv.two_tone(x, base, 11, (tier_color(&rank.tier), 700, &lang.rank_name(rank)), ("#9aa6c0", &format!("  ·  {} LP", rank.lp)));
    x += text_width(&lang.rank(rank), 11) * 7 / 8 + 14;
    if let (true, Some(delta)) = (p.tracked, p.lp_delta) {
        let (fill, ink) = if delta > 0 { ("#12382f", GREEN) } else { ("#431f2d", RED) };
        let label = format!("{delta:+} LP");
        let width = text_width(&label, 10) + 10;
        cv.rect(x, base - 12, width, 16, 2, fill);
        cv.text(x + width / 2, base, 10, ink, 700, "middle", &label);
    }
}

fn items(cv: &mut Canvas, p: &Participant, y: u32, art: &Assets) {
    for (slot, item) in p.items.iter().take(7).enumerate() {
        let x = ITEMS_X + slot as u32 * 32;
        let radius = if slot == 6 { 14 } else { 2 };
        if *item > 0 { cv.image(art.image(&format!("item:{item}")), x, y, 29, 29, radius); } else { cv.rect(x, y, 29, 29, radius, "#0c1320"); }
        cv.outline(x, y, 29, 29, radius, "#243249");
    }
}
