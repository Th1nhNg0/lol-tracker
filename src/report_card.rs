//! Daily/weekly/monthly report card: group LP, a few honours and one LP bar per player.
use crate::{App, i18n::{Lang, Msg}, render::{BLUE, Canvas, tier_color, DIM, FAINT, GOLD, GREEN, HERO_DEFS, RED, TEXT, blocking, rasterize, short},
    render_assets::{Assets, Wanted}, reports::{PlayerLine, Report}};
use anyhow::{Result, ensure};
use std::path::Path;

const WIDTH: u32 = 1200;
const HERO: u32 = 236;
const AWARD_COLORS: [&str; 6] = [GOLD, RED, "#ff9e46", GREEN, "#50d7e6", BLUE];
const ROW: u32 = 72;
const MARGIN: u32 = 32;
/// Zero line of the LP bars and the longest bar in either direction.
const ZERO_X: u32 = 660;
const SPAN: u32 = 190;

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
        let (gap, height) = (14, 88);
        let width = (WIDTH - 2 * MARGIN - 2 * gap) / 3;
        for (index, award) in awards.iter().enumerate() {
            let x = MARGIN + (index as u32 % 3) * (width + gap);
            let top = y + (index as u32 / 3) * (height + gap);
            let color = if award.negative { RED } else { AWARD_COLORS[index % AWARD_COLORS.len()] };
            let player = &report.players[award.player];
            cv.rect(x, top, width, height, 16, "#142033");
            cv.outline(x, top, width, height, 16, "#273144");
            cv.rect(x + 18, top + 22, 16, 3, 1, color);
            cv.text(x + 42, top + 28, 12, color, 700, "start", &lang.t(award.title).to_uppercase());
            cv.text(x + 18, top + 56, 20, TEXT, 700, "start", &short(&player.name, 17));
            cv.text(x + 18, top + 77, 13, DIM, 400, "start", &award.value);
            cv.image(portrait(art, player), x + width - 70, top + 18, 52, 52, 14);
            cv.outline(x + width - 70, top + 18, 52, 52, 14, color);
        }
        y += awards.len().div_ceil(3) as u32 * (height + gap) + 8;
    }
    let widest = report.players.iter().filter_map(PlayerLine::lp).map(i64::abs).max().unwrap_or(0).max(1);
    let last = report.players.len() - 1;
    for (index, p) in report.players.iter().enumerate() {
        let lp = p.lp();
        // The biggest climber and the biggest drop get a coloured frame.
        let frame = match lp { Some(d) if index == 0 && d > 0 => Some(GREEN), Some(d) if index == last && index > 0 && d < 0 => Some(RED), _ => None };
        cv.rect(MARGIN, y, WIDTH - 2 * MARGIN, ROW - 8, 16, "#131d2f");
        if let Some(color) = frame { cv.outline(MARGIN, y, WIDTH - 2 * MARGIN, ROW - 8, 16, color); }
        cv.image(portrait(art, p), 50, y + 8, 48, 48, 12);
        cv.outline(50, y + 8, 48, 48, 12, frame.unwrap_or("#2c3a50"));
        cv.text(116, y + 30, 19, TEXT, 700, "start", &short(&p.name, 22));
        cv.text(116, y + 51, 12, DIM, 400, "start", &lang.f(Msg::FmtWinRate, &[&format!("{:.0}", p.win_rate()), &p.games]));
        cv.rect(ZERO_X, y + 12, 2, ROW - 32, 1, "#2a3549");
        match lp {
            Some(0) => cv.text(ZERO_X + 14, y + 40, 22, DIM, 700, "start", "0"),
            Some(delta) => {
                let length = (delta.unsigned_abs() as f64 / widest as f64 * f64::from(SPAN)).round().max(8.0) as u32;
                let (color, label, radius) = (if delta < 0 { RED } else { GREEN }, format!("{delta:+}"), (length / 2).min(12));
                if delta < 0 {
                    cv.rect(ZERO_X + 2 - length, y + 20, length, 24, radius, color);
                    cv.text(ZERO_X - length - 12, y + 40, 22, color, 700, "end", &label);
                } else {
                    cv.rect(ZERO_X, y + 20, length, 24, radius, color);
                    cv.text(ZERO_X + length + 12, y + 40, 22, color, 700, "start", &label);
                }
            }
            None => cv.text(ZERO_X + 14, y + 37, 13, FAINT, 400, "start", lang.t(Msg::LpUnknown)),
        }
        // Current rank in the queue played most.
        if let Some(line) = p.ranks.iter().filter(|r| r.rank.is_some()).max_by_key(|r| r.games)
            && let Some(rank) = &line.rank
        {
            cv.text(WIDTH - 50, y + 30, 15, tier_color(&rank.tier), 700, "end", &lang.rank(rank));
            cv.text(WIDTH - 50, y + 51, 12, FAINT, 400, "end", &lang.queue(line.queue));
        }
        y += ROW;
    }
    let height = y + 20;
    let svg = format!("<svg xmlns='http://www.w3.org/2000/svg' width='{WIDTH}' height='{height}'>{HERO_DEFS}<g font-family='Be Vietnam Pro,DejaVu Sans,sans-serif'><rect width='{WIDTH}' height='{height}' fill='#0b1220'/>{}</g></svg>", cv.svg);
    ensure!(height <= 8000, "Report exceeds image bounds");
    rasterize(&svg, WIDTH, height)
}

fn hero(cv: &mut Canvas, report: &Report, lang: Lang, art: &Assets) -> u32 {
    if let Some(splash) = art.image("splash") {
        cv.image(Some(splash), 0, 0, WIDTH, HERO, 0);
    } else { cv.rect(0, 0, WIDTH, HERO, 0, "#182334"); }
    cv.rect(0, 0, WIDTH, HERO, 0, "url(#heroShade)");
    cv.rect(0, HERO - 110, WIDTH, 110, 0, "url(#fade)");
    let net = report.net_lp();
    let accent = net.map_or(BLUE, |n| if n >= 0 { GREEN } else { RED });
    cv.rect(0, 36, 5, 150, 0, accent);
    cv.text(54, 56, 13, GOLD, 700, "start", lang.t(Msg::ReportBadge));
    cv.text(50, 124, 54, accent, 700, "start", &short(&report.title(lang).to_uppercase(), 24));
    cv.text(54, 162, 22, TEXT, 700, "start", &report.range(lang));
    cv.text(54, 192, 15, DIM, 400, "start", &lang.f(Msg::MatchesCount, &[&report.matches]));
    let left = WIDTH - 372;
    cv.rect(left, 48, 320, 136, 22, "rgba(8,14,25,.82)");
    cv.outline(left, 48, 320, 136, 22, "#39465a");
    cv.text(left + 160, 82, 12, DIM, 700, "middle", lang.t(Msg::GroupLp));
    cv.text(left + 160, 148, 54, accent, 700, "middle", &net.map_or_else(|| "—".to_owned(), |n| format!("{n:+} LP")));
    HERO + 16
}
