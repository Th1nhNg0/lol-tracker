//! Smoke tests for the image renderers: every layout must produce a decodable WebP at the expected size.
use crate::{i18n::Lang, model::{Match, Participant, Rank}, render_assets::Assets, report_card, reports::{Period, PlayerLine, RankLine, Report}, scoreboard};
use std::collections::BTreeMap;

fn rank(tier: &str, division: &str, lp: i64) -> Option<Rank> { Some(Rank { queue: 440, tier: tier.into(), division: division.into(), lp }) }

fn player(id: u64, team: u16, win: bool, tracked: bool) -> Participant {
    let roles = ["TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY"];
    Participant {
        id, name: format!("Player {id} with a rather long name"), champion: "Ahri".into(), team, role: roles[(id as usize - 1) % 5].into(), tracked, win,
        kills: id as i64, deaths: (id as i64 * 3) % 7, assists: 5 + id as i64, cs: 120 + id as i64 * 11, damage: 9_000 + id as i64 * 2_500, gold: 8_000 + id as i64 * 900,
        vision: 10 + id as i64, level: 9 + id as i64 % 9, damage_taken: 12_000 + id as i64 * 1_800, items: vec![3157, 3089, 0, 3020, 0, 0, 3340],
        rank_after: if tracked { rank("DIAMOND", "II", 40) } else { None }, lp_delta: tracked.then_some(if win { 19 } else { -17 }), ..Default::default()
    }
}

fn game(teams: u16, per_team: u64, remake: bool) -> Match {
    let mut participants = Vec::new();
    for t in 0..teams {
        for i in 0..per_team {
            let team = if teams == 2 { 100 + t * 100 } else { t + 1 };
            participants.push(player(t as u64 * per_team + i + 1, team, t == 0, i == 0));
        }
    }
    let objectives = (teams == 2).then(|| [("100".to_owned(), BTreeMap::from([("dragon".to_owned(), 3), ("tower".to_owned(), 7)])), ("200".to_owned(), BTreeMap::from([("baron".to_owned(), 1)]))].into()).unwrap_or_default();
    Match { id: "TEST_1".into(), queue: 440, started_at: 1_700_000_000, ended_at: 1_700_001_800, duration: 1800, remake, participants, objectives }
}

/// Decodes the WebP and checks it is a 2x render of the 1200 px layout.
fn assert_webp(bytes: &[u8]) {
    let image = webp::Decoder::new(bytes).decode().expect("output is a valid WebP");
    assert_eq!(image.width(), 2400);
    assert!(image.height() > 600 && image.height() < 12_000, "unexpected height {}", image.height());
    assert!(bytes.len() < 3 * 1024 * 1024, "image is {} bytes", bytes.len());
}

#[test]
fn scoreboard_renders_every_layout() {
    let art = Assets::default();
    for (name, teams, per_team, remake) in [("5v5", 2, 5, false), ("remake", 2, 5, true), ("arena", 8, 2, false), ("one team", 1, 5, false)] {
        for lang in Lang::ALL {
            let image = scoreboard::scoreboard(&game(teams, per_team, remake), lang, "Asia/Ho_Chi_Minh", true, &art).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_webp(&image);
        }
    }
}

#[test]
fn scoreboard_rejects_bad_input() {
    let art = Assets::default();
    assert!(scoreboard::scoreboard(&game(2, 5, false), Lang::En, "Not/AZone", false, &art).is_err());
    assert!(scoreboard::scoreboard(&Match { participants: Vec::new(), ..game(2, 5, false) }, Lang::En, "UTC", false, &art).is_err());
}

fn line(name: &str, games: i64, wins: i64, delta: Option<i64>) -> PlayerLine {
    PlayerLine {
        name: name.into(), games, wins, kills: games * 7, deaths: games * 5, assists: games * 9, cs: games * 180, damage: games * 21_000, vision: games * 30, minutes: games as f64 * 30.0,
        champions: vec![("Ahri".into(), games)], ranks: vec![RankLine { queue: 440, games, wins, delta, rank: rank("EMERALD", "I", 35) }],
    }
}

#[test]
fn report_card_renders_every_block() {
    let art = Assets::default();
    let full = Report { period: Period::Weekly, start: 1_700_000_000, end: 1_700_600_000, timezone: "UTC".into(), matches: 12, partial_lp: false,
        players: vec![line("Climber", 10, 7, Some(64)), line("Flat", 4, 2, Some(0)), line("Dropper", 8, 3, Some(-41)), line("Unknown", 3, 1, None)] };
    let small = Report { players: vec![line("Only", 1, 1, Some(12))], ..full.clone() };
    for report in [full, small] {
        for lang in Lang::ALL {
            assert_webp(&report_card::card(&report, lang, &art).expect("report card"));
        }
    }
}
