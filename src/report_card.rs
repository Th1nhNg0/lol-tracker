//! Daily/weekly/monthly report ard: group LP, honours, per-player statistics and ranks, pairs.
use crate::{App, i18n::{Lang, Msg}, render::{BLUE, Canvas, DIM, FAINT, GOLD, GREEN, HERO_DEFS, RED, TEXT, WIDTH, blocking, compact, rasterize, short},
    render_assets::{Assets, Wanted}, reports::{PlayerLine, Report}};
use anyhow::{Result, ensure};
use std::path::Path;

const HERO: u32 = 316;
const ROW: u32 = 80;
const QUEUES: [u16; 2] = [420, 440];
const QUEUE_X: [u32; 2] = [1184, 1432];
const AWARD_COLORS: [&str; 6] = [GOLD, RED, "#ff9e46", GREEN, "#50d7e6", BLUE];

fn tier_color(tier: &str) -> &'static str {
    match tier {
        "IRON" => "#9a9a9a", "BRONZE" => "#c58b65", "SILVER" => "#b8c4d0", "GOLD" => "#e6c06a",
        "PLATINUM" => "#4fc1b8", "EMERALD" => "#3fcf86", "DIAMOND" => "#7aa8ff", "MASTER" => "#c27cf0",
        "GRANDMASTER" => "#ff6b6b", "CHALLENGER" => "#f5d77a", _ => DIM,
    }
}
fn delta_color(delta: i64) -> &'static str { if delta > 0 { GREEN } else if delta < 0 { RED } else { DIM } }
fn top_champion(p: &PlayerLine) -> Option<&str> { p.champions.first().map(|(c, _)| c.as_str()) }
fn portrait<'a>(art: &'a Assets, p: &PlayerLine) -> Option<&'a str> { top_champion(p).and_then(|c| art.image(&format!("champion:{c}"))) }

pub async fn png(app: &App, report: Report, lang: Lang) -> Result<Vec<u8>> {
    let wanted = Wanted {
        champions: report.players.iter().filter_map(top_champion).map(str::to_owned).collect(),
        splash: report.players.first().and_then(top_champion).map(str::to_owned),
        ..Default::default()
    };
    let art = Assets::load(wanted, app.cfg.database.parent().unwrap_or(Path::new(".")).join("artwork")).await;
    blocking(move || card(&report, lang, &art)).await
}

pub fn card(report: &Report, lang: Lang, art: &Assets) -> Result<Vec<u8>> {
    ensure!(!report.players.is_empty() && report.players.len() <= 100, "Unsupported report size");
    let mut cv = Canvas { svg: String::new(), clips: 0 };
    let mut y = hero(&mut cv, report, lang, art);
    let awards = report.awards(lang);
    if !awards.is_empty() {
        y = section(&mut cv, y, lang.t(Msg::HonoursSection), "", GOLD);
        let (width, height, gap) = (501, 104, 16);
        for (index, award) in awards.iter().enumerate() {
            let x = 32 + (index as u32 % 3) * (width + gap);
            let top = y + (index as u32 / 3) * (height + gap);
            let color = if award.negative { RED } else { AWARD_COLORS[index % AWARD_COLORS.len()] };
            let player = &report.players[award.player];
            cv.rect(x, top, width, height, 16, "#182131");
            cv.outline(x, top, width, height, 16, "#273144");
            cv.rect(x + 20, top + 23, 18, 3, 1, color);
            cv.text(x + 48, top + 29, 13, color, 700, "start", &lang.t(award.title).to_uppercase());
            cv.text(x + 20, top + 63, 22, TEXT, 700, "start", &short(&player.name, 24));
            cv.text(x + 20, top + 88, 14, DIM, 400, "start", &award.value);
            cv.image(portrait(art, player), x + width - 76, top + 24, 56, 56, 14);
            cv.outline(x + width - 76, top + 24, 56, 56, 14, color);
        }
        y += awards.len().div_ceil(3) as u32 * (height + gap) + 14;
    }
    y = table(&mut cv, y, report, lang, art);
    if !report.pairs.is_empty() {
        y = section(&mut cv, y, lang.t(Msg::PairsSection), lang.t(Msg::PairsDetail), BLUE);
        let (width, height, gap) = (760, 66, 16);
        for (index, pair) in report.pairs.iter().enumerate() {
            let x = 32 + (index as u32 % 2) * (width + gap);
            let top = y + (index as u32 / 2) * (height + 10);
            let rate = pair.wins as f64 / pair.games.max(1) as f64;
            cv.rect(x, top, width, height, 14, "#182131");
            cv.outline(x, top, width, height, 14, "#273144");
            cv.text(x + 22, top + 29, 17, TEXT, 700, "start", &format!("{} + {}", short(&pair.a, 18), short(&pair.b, 18)));
            cv.text(x + 22, top + 51, 12, DIM, 400, "start", &lang.f(Msg::WinsOfGames, &[&pair.wins, &pair.games]));
            cv.rect(x + 450, top + 29, 220, 8, 4, "#344056");
            cv.rect(x + 450, top + 29, (rate * 220.0).round() as u32, 8, 4, if rate >= 0.5 { GREEN } else { RED });
            cv.text(x + width - 22, top + 38, 17, if rate >= 0.5 { GREEN } else { RED }, 700, "end", &format!("{:.0}%", rate * 100.0));
        }
        y += report.pairs.len().div_ceil(2) as u32 * (height + 10) + 14;
    }
    if report.partial_lp {
        cv.text(32, y + 12, 13, DIM, 400, "start", lang.t(Msg::PartialLp));
        y += 28;
    }
    let height = y + 44;
    cv.rect(32, y + 4, WIDTH - 64, 1, 0, "#273144");
    cv.text(32, y + 28, 12, FAINT, 400, "start", lang.t(Msg::ImgFooter));
    cv.text(WIDTH - 32, y + 28, 12, GOLD, 700, "end", "LOL TRACKER");
    let svg = format!("<svg xmlns='http://www.w3.org/2000/svg' width='{WIDTH}' height='{height}'>{HERO_DEFS}<g font-family='Be Vietnam Pro,DejaVu Sans,sans-serif'><rect width='{WIDTH}' height='{height}' fill='#0b1220'/>{}</g></svg>", cv.svg);
    ensure!(height <= 8000, "Report exceeds image bounds");
    rasterize(&svg, WIDTH, height)
}

fn hero(cv: &mut Canvas, report: &Report, lang: Lang, art: &Assets) -> u32 {
    if let Some(splash) = art.image("splash") {
        cv.image(Some(splash), 0, 0, WIDTH, HERO - 16, 0);
    } else { cv.rect(0, 0, WIDTH, HERO - 16, 0, "#182334"); }
    cv.rect(0, 0, WIDTH, HERO - 16, 0, "url(#heroShade)");
    cv.rect(0, HERO - 140, WIDTH, 124, 0, "url(#fade)");
    let net = report.net_lp();
    let accent = net.map_or(BLUE, |n| if n >= 0 { GREEN } else { RED });
    cv.rect(0, 32, 5, 225, 0, accent);
    cv.text(54, 44, 13, GOLD, 700, "start", lang.t(Msg::ReportBadge));
    cv.text(50, 112, 52, accent, 700, "start", &short(&report.title(lang).to_uppercase(), 34));
    cv.text(54, 152, 22, TEXT, 700, "start", &report.range(lang));
    let appearances: i64 = report.players.iter().map(|p| p.games).sum();
    cv.text(54, 184, 16, DIM, 400, "start", &format!("{}  ·  {}", lang.f(Msg::MatchesCount, &[&report.matches]), lang.f(Msg::PlayerGames, &[&appearances])));
    let shown: Vec<_> = report.players.iter().take(10).collect();
    for (i, p) in shown.iter().enumerate() {
        let x = 54 + i as u32 * 43;
        cv.image(portrait(art, p), x, 207, 47, 47, 23);
        cv.outline(x, 207, 47, 47, 23, "#0b1220");
    }
    cv.text(54 + shown.len() as u32 * 43 + 21, 236, 16, TEXT, 700, "start", &lang.f(Msg::PlayersCount, &[&report.players.len()]));
    cv.rect(1142, 52, 402, 139, 20, "rgba(8,14,25,.84)");
    cv.outline(1142, 52, 402, 139, 20, "#39465a");
    cv.text(1343, 84, 13, DIM, 700, "middle", lang.t(Msg::GroupLp));
    cv.text(1343, 158, 58, accent, 700, "middle", &net.map_or_else(|| "—".to_owned(), |n| format!("{n:+} LP")));
    let (gained, lost) = report.gained_lost();
    if gained + lost > 0 {
        let share = (gained as f64 / (gained + lost) as f64 * 1492.0).round() as u32;
        cv.rect(54, 268, 1492, 8, 4, RED);
        cv.rect(54, 268, share, 8, 4, GREEN);
        cv.text(54, 296, 13, GREEN, 700, "start", &lang.f(Msg::LpGained, &[&gained]));
        cv.text(1546, 296, 13, RED, 700, "end", &lang.f(Msg::LpLost, &[&lost]));
    }
    HERO + 14
}

fn section(cv: &mut Canvas, y: u32, title: &str, detail: &str, color: &str) -> u32 {
    cv.rect(32, y, WIDTH - 64, 52, 13, "#142033");
    cv.outline(32, y, WIDTH - 64, 52, 13, "#2b3a52");
    cv.rect(32, y + 8, 4, 36, 2, color);
    cv.text(54, y + 34, 21, color, 700, "start", &title.to_uppercase());
    if !detail.is_empty() { cv.text(WIDTH - 56, y + 33, 14, DIM, 400, "end", detail); }
    y + 66
}

fn table(cv: &mut Canvas, y: u32, report: &Report, lang: Lang, art: &Assets) -> u32 {
    let mut y = section(cv, y, lang.t(Msg::TableSection), lang.t(Msg::TableDetail), GREEN);
    for (x, label) in [(470, lang.t(Msg::ColGames)), (590, "KDA"), (710, lang.t(Msg::ColCsMin)), (830, lang.t(Msg::ImgDamage)), (950, lang.t(Msg::ImgVision))] {
        cv.text(x, y + 6, 11, FAINT, 700, "middle", label);
    }
    cv.text(120, y + 6, 11, FAINT, 700, "start", lang.t(Msg::ColPlayer));
    for (queue, x) in QUEUES.iter().zip(QUEUE_X) { cv.text(x, y + 6, 13, GREEN, 700, "middle", &lang.queue(*queue)); }
    y += 20;
    for p in &report.players {
        let lp = p.lp();
        let losing = lp.is_some_and(|d| d < 0);
        cv.rect(32, y, WIDTH - 64, 72, 13, if losing { "#2a1f2b" } else { "#172438" });
        cv.outline(32, y, WIDTH - 64, 72, 13, if losing { "#633447" } else { "#34546f" });
        cv.image(portrait(art, p), 49, y + 9, 54, 54, 11);
        cv.outline(49, y + 9, 54, 54, 11, if losing { RED } else { BLUE });
        cv.text(120, y + 30, 19, TEXT, 700, "start", &short(&p.name, 20));
        let subtitle = p.champions.first().map(|(c, n)| format!("{} ×{n}", art.names.get(c).map_or(c.as_str(), String::as_str))).unwrap_or_default();
        cv.text(120, y + 54, 12, DIM, 400, "start", &short(&subtitle, 22));
        if let Some(delta) = lp {
            let label = format!("{delta:+} LP");
            let width = 16 + label.chars().count() as u32 * 8;
            cv.rect(412 - width, y + 40, width, 21, 10, if delta < 0 { "#4a2534" } else { "#183f37" });
            cv.text(412 - width / 2, y + 55, 12, delta_color(delta), 700, "middle", &label);
        }
        cv.text(470, y + 31, 22, TEXT, 700, "middle", &p.games.to_string());
        let record = format!("{}-{} · {:.0}%", p.wins, p.games - p.wins, p.win_rate());
        cv.text(470, y + 55, 11, if p.wins * 2 >= p.games { GREEN } else { RED }, 700, "middle", &record);
        let average = |v: i64| v as f64 / p.games.max(1) as f64;
        for (x, value, detail) in [
            (590, format!("{:.2}", p.kda()), format!("{:.1}/{:.1}/{:.1}", average(p.kills), average(p.deaths), average(p.assists))),
            (710, format!("{:.1}", p.cs_per_min()), format!("{:.0}{}", average(p.cs), lang.t(Msg::PerGame))),
            (830, compact(p.damage_per_game().round() as i64), lang.t(Msg::PerGame).to_owned()),
            (950, format!("{:.2}", p.vision_per_min()), lang.t(Msg::PerMinute).to_owned()),
        ] {
            cv.text(x, y + 31, 20, TEXT, 700, "middle", &value);
            cv.text(x, y + 55, 11, FAINT, 400, "middle", &detail);
        }
        for (queue, x) in QUEUES.iter().zip(QUEUE_X) {
            match p.ranks.iter().find(|r| r.queue == *queue) {
                Some(line) => {
                    match &line.rank {
                        Some(rank) => cv.text(x, y + 28, 13, tier_color(&rank.tier), 700, "middle", &lang.rank(rank)),
                        None => cv.text(x, y + 28, 13, FAINT, 400, "middle", lang.t(Msg::RankNoData)),
                    }
                    match line.delta {
                        Some(delta) => cv.text(x, y + 56, 20, delta_color(delta), 700, "middle", &format!("{delta:+} LP")),
                        None => cv.text(x, y + 54, 13, FAINT, 400, "middle", lang.t(Msg::LpUnknown)),
                    }
                }
                None => cv.text(x, y + 43, 20, FAINT, 400, "middle", "–"),
            }
        }
        y += ROW;
    }
    y + 6
}

