//! Daily/weekly/monthly report card in the scoreboard's style: a header, honours, then a tinted block
//! of players who climbed and one of players who dropped.
use crate::{App, i18n::{Lang, Msg}, render::{Canvas, DIM, GOLD, GREEN, RED, TEXT, blocking, number, rasterize, short, text_width}, render_assets::{Assets, Wanted},
    reports::{PlayerLine, Report}, scoreboard::{Block, DEFEAT, VICTORY, tier_gem}};
use anyhow::{Result, ensure};
use std::path::Path;

const W: u32 = 1200;
const HEAD: u32 = 56;
const TEAM_H: u32 = 36;
const ROW: u32 = 58;
const TILE_H: u32 = 70;
const NAME_X: u32 = 70;
const RECORD_X: u32 = 392;
const KDA_X: u32 = 486;
const DAMAGE_X: u32 = 570;
const BAR_W: u32 = 118;
const CS_X: u32 = 776;
const VISION_X: u32 = 864;
const LP_X: u32 = 930;
const LP_SPAN: u32 = 190;
const MUTED: &str = "#9aa6c0";
const NEUTRAL: Block = Block { header: "#1a2030", title: "#aab4c8", rows: ["#161d2e", "#1a2236"], tracked: "#1a2236" };
const TILE_COLORS: [&str; 6] = [GOLD, RED, "#ff9f43", GREEN, "#4fd1e6", "#6fa0ff"];

fn top_champion(p: &PlayerLine) -> Option<&str> { p.champions.first().map(|(c, _)| c.as_str()) }
fn portrait<'a>(art: &'a Assets, p: &PlayerLine) -> Option<&'a str> { top_champion(p).and_then(|c| art.image(&format!("champion:{c}"))) }

pub async fn png(app: &App, report: Report, lang: Lang) -> Result<Vec<u8>> {
    let wanted = Wanted { champions: report.players.iter().filter_map(top_champion).map(str::to_owned).collect(), ..Default::default() };
    let art = Assets::load(wanted, app.cfg.database.parent().unwrap_or(Path::new(".")).join("artwork")).await;
    blocking(move || card(&report, lang, &art)).await
}

pub fn card(report: &Report, lang: Lang, art: &Assets) -> Result<Vec<u8>> {
    ensure!(!report.players.is_empty() && report.players.len() <= 100, "Unsupported report size");
    let awards = report.awards(lang);
    let climbed: Vec<&PlayerLine> = report.players.iter().filter(|p| p.lp().is_some_and(|d| d >= 0)).collect();
    // Biggest drop first.
    let mut dropped: Vec<&PlayerLine> = report.players.iter().filter(|p| p.lp().is_some_and(|d| d < 0)).collect();
    dropped.sort_by_key(|p| p.lp());
    let unknown: Vec<&PlayerLine> = report.players.iter().filter(|p| p.lp().is_none()).collect();
    let blocks = [(lang.t(Msg::BlockClimbed), &VICTORY, climbed), (lang.t(Msg::BlockDropped), &DEFEAT, dropped), (lang.t(Msg::BlockUnknown), &NEUTRAL, unknown)];
    let honours_h = if awards.is_empty() { 0 } else { TEAM_H + 2 * TILE_H };
    let height = HEAD + honours_h + blocks.iter().filter(|(_, _, rows)| !rows.is_empty()).map(|(_, _, rows)| TEAM_H + ROW * rows.len() as u32).sum::<u32>();
    ensure!(height <= 8000, "Report exceeds image bounds");
    let mut cv = Canvas { svg: format!("<svg xmlns='http://www.w3.org/2000/svg' width='{W}' height='{height}'><g font-family='Be Vietnam Pro,DejaVu Sans,sans-serif'>"), clips: 0 };
    cv.rect(0, 0, W, height, 0, "#0b1020");
    header(&mut cv, report, lang);
    let mut y = HEAD;
    if !awards.is_empty() {
        cv.rect(0, y, W, TEAM_H, 0, "#1a2030");
        cv.text(18, y + 24, 14, "#c4cddb", 700, "start", &lang.t(Msg::HonoursSection).to_uppercase());
        y += TEAM_H;
        let width = W / 3;
        for (index, award) in awards.iter().enumerate() {
            let (x, top) = (index as u32 % 3 * width, y + index as u32 / 3 * TILE_H);
            let ink = if award.negative { RED } else { TILE_COLORS[index % TILE_COLORS.len()] };
            let player = &report.players[award.player];
            cv.rect(x, top, width, TILE_H, 0, if (index + index / 3) % 2 == 0 { "#161d2e" } else { "#1a2236" });
            cv.rect(x, top, 4, TILE_H, 0, ink);
            cv.text(x + 20, top + 24, 11, ink, 700, "start", &lang.t(award.title).to_uppercase());
            cv.text(x + 20, top + 46, 16, TEXT, 700, "start", &short(&player.name, 18));
            cv.text(x + 20, top + 63, 12, MUTED, 400, "start", &award.value);
            cv.image(portrait(art, player), x + width - 62, top + 12, 46, 46, 2);
            cv.outline(x + width - 62, top + 12, 46, 46, 2, ink);
        }
        y += 2 * TILE_H;
    }
    let max_damage = report.players.iter().map(|p| p.damage_per_game().round() as i64).max().unwrap_or(1).max(1);
    let widest = report.players.iter().filter_map(PlayerLine::lp).map(i64::abs).max().unwrap_or(0).max(1);
    for (title, block, rows) in &blocks {
        if rows.is_empty() { continue; }
        cv.rect(0, y, W, TEAM_H, 0, block.header);
        cv.text(18, y + 24, 14, block.title, 700, "start", &format!("{title}  ·  {}", rows.len()));
        for (x, label) in [(RECORD_X, lang.t(Msg::ColRecord)), (KDA_X, "KDA"), (DAMAGE_X + BAR_W / 2, lang.t(Msg::ColDamageGame)), (CS_X, lang.t(Msg::ColCsMin)), (VISION_X, lang.t(Msg::ColVisionMin)), (LP_X + 90, lang.t(Msg::ColLpChange))] {
            cv.text(x, y + 23, 10, "#8b97b3", 700, "middle", label);
        }
        for (i, p) in rows.iter().enumerate() {
            let ry = y + TEAM_H + i as u32 * ROW;
            cv.rect(0, ry, W, ROW, 0, block.rows[i % 2]);
            cv.image(portrait(art, p), 14, ry + 6, 46, 46, 2);
            cv.outline(14, ry + 6, 46, 46, 2, "#2a374d");
            cv.text(NAME_X, ry + 24, 14, TEXT, 700, "start", &short(&p.name, 22));
            if let Some(line) = p.ranks.iter().filter(|r| r.rank.is_some()).max_by_key(|r| r.games) && let Some(rank) = &line.rank {
                tier_gem(&mut cv, NAME_X, ry + 44, &rank.tier);
                let text = format!("{}  ·  {}", lang.rank(rank), lang.queue(line.queue));
                cv.text(NAME_X + 16, ry + 44, 11, DIM, 400, "start", &text);
            }
            cv.text(RECORD_X, ry + 24, 14, TEXT, 700, "middle", &format!("{} – {}", p.wins, p.games - p.wins));
            cv.text(RECORD_X, ry + 43, 12, if p.wins * 2 >= p.games { GREEN } else { RED }, 700, "middle", &format!("{:.0}%", p.win_rate()));
            let kda = p.kda();
            cv.text(KDA_X, ry + 24, 14, if kda >= 3.0 { "#ff9f43" } else { TEXT }, 700, "middle", &format!("{kda:.2}:1"));
            let average = |v: i64| v as f64 / p.games.max(1) as f64;
            cv.text(KDA_X, ry + 43, 12, MUTED, 400, "middle", &format!("{:.1}/{:.1}/{:.1}", average(p.kills), average(p.deaths), average(p.assists)));
            let damage = p.damage_per_game().round() as i64;
            cv.text(DAMAGE_X, ry + 24, 13, TEXT, 400, "start", &number(damage));
            cv.rect(DAMAGE_X, ry + 32, BAR_W, 7, 0, "#1b2639");
            cv.rect(DAMAGE_X, ry + 32, (damage as f64 / max_damage as f64 * f64::from(BAR_W)).clamp(2.0, f64::from(BAR_W)) as u32, 7, 0, "#3d7cff");
            cv.text(CS_X, ry + 34, 14, TEXT, 700, "middle", &format!("{:.1}", p.cs_per_min()));
            cv.text(VISION_X, ry + 34, 14, TEXT, 700, "middle", &format!("{:.2}", p.vision_per_min()));
            match p.lp() {
                Some(0) => cv.text(LP_X, ry + 34, 16, DIM, 700, "start", "0"),
                Some(delta) => {
                    let length = (delta.unsigned_abs() as f64 / widest as f64 * f64::from(LP_SPAN)).round().max(6.0) as u32;
                    let ink = if delta < 0 { "#e84057" } else { "#3d7cff" };
                    cv.rect(LP_X, ry + 22, length, 22, 0, ink);
                    cv.text(LP_X + length + 10, ry + 39, 16, if delta < 0 { RED } else { GREEN }, 700, "start", &format!("{delta:+}"));
                }
                None => cv.text(LP_X, ry + 34, 12, MUTED, 400, "start", lang.t(Msg::LpUnknown)),
            }
        }
        y += TEAM_H + ROW * rows.len() as u32;
    }
    cv.svg.push_str("</g></svg>");
    rasterize(&cv.svg, W, height)
}

fn header(cv: &mut Canvas, report: &Report, lang: Lang) {
    let net = report.net_lp();
    let accent = net.map_or(DIM, |n| if n >= 0 { GREEN } else { RED });
    let title = short(&report.title(lang).to_uppercase(), 28);
    cv.rect(0, 0, W, HEAD, 0, "#0f1424");
    cv.text(18, 37, 22, accent, 700, "start", &title);
    let sub = format!("{}  ·  {}", report.range(lang), lang.f(Msg::MatchesCount, &[&report.matches]));
    cv.text(18 + text_width(&title, 22) * 8 / 7 + 18, 36, 13, "#c4cddb", 400, "start", &sub);
    cv.text(W - 18, 40, 11, "#8b97b3", 700, "end", lang.t(Msg::GroupLp));
    cv.text(W - 18 - text_width(lang.t(Msg::GroupLp), 11) - 14, 40, 30, accent, 700, "end", &net.map_or_else(|| "—".to_owned(), |n| format!("{n:+} LP")));
}
