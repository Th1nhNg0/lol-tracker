//! Opt-in offline memory workload using a real SQLite history and production report path.
use crate::{App, config::Config, i18n::Lang, reports::{self, Period}, riot::Riot, store::Store};

#[test]
#[ignore = "opt-in release-mode SQLite report memory workload"]
fn memory_stored_reports() {
    let database = std::path::PathBuf::from(std::env::var_os("RIFT_RECAP_BENCH_DATABASE").expect("provide an isolated fixture database"));
    let end = 1_711_929_599; // 2024-03-31 23:59:59 UTC
    let mut cfg = Config { database: database.clone(), ..Config::default() };
    cfg.reports.timezone = "UTC".into();
    let app = App { cfg, store: Store::open(&database).unwrap(), riot: Riot::new("offline-fixture-not-a-key", "vn2", 1250).unwrap(), llm: None };
    for round in 0..2 {
        for period in [Period::Daily, Period::Weekly, Period::Monthly] {
            let report = reports::build_at(&app, period, end).unwrap().expect("fixture contains tracked games");
            assert!(report.matches > 0 && !report.players.is_empty());
            let output = std::path::PathBuf::from(std::env::var_os("RIFT_RECAP_BENCH_OUTPUT").expect("provide an isolated output directory"));
            std::fs::write(output.join(format!("{}-{round}.json", period.key())), serde_json::to_vec(&report).unwrap()).unwrap();
            for lang in Lang::ALL {
                std::fs::write(output.join(format!("{}-{}-{round}.txt", period.key(), lang.code())), report.text(lang)).unwrap();
            }
        }
    }
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        if let Some(line) = status.lines().find(|line| line.starts_with("VmRSS:")) { println!("AFTER_REPORT {line}"); }
    }
}

#[test]
fn streamed_reports_match_materialized_windows() {
    use crate::model::{Match, Participant, Player, Rank};
    use chrono::{Datelike, TimeZone};
    let end = 1_711_929_599;
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    for i in 0..4 {
        store.add_player(&Player { game_name: format!("Player{i}"), tag_line: "TEST".into(), nickname: format!("Nguyễn {i}"), puuid: format!("puuid{i}"), baselined: true }).unwrap();
        for queue in [420, 440] {
            if i == 3 { continue; }
            for (at, lp) in [(end - if i == 1 { 5 } else { 32 } * 86400, 35), (end - 60, 40 + i), (end + 1, 99)] {
                store.snapshot(&format!("puuid{i}"), &Rank { queue, tier: "EMERALD".into(), division: "II".into(), lp }, at).unwrap();
            }
        }
    }
    let mut app = App { cfg: Config::default(), store, riot: Riot::new("offline-fixture-not-a-key", "vn2", 1250).unwrap(), llm: None };
    // Include exact endpoints, remakes, empty tracked sets, removed players,
    // both ranked queues, unranked games and champion ties.
    for (i, at) in [end - 32 * 86400, end - 30 * 86400, end - 7 * 86400, end - 86400, end - 60, end, end + 1].into_iter().enumerate() {
        let participants = (0..4).map(|p| Participant {
            id: p + 1, puuid: format!("puuid{p}"), name: format!("Old name{p}"), tracked: i != 2, champion: if i % 2 == 0 { "Ahri" } else { "Lux" }.into(), team: 100, win: i % 2 == 0,
            kills: 5, deaths: 3, assists: 9, damage: 12_000, cs: 150, vision: 25, ..Default::default()
        }).collect();
        app.store.finish(&Match { id: format!("TEST_{i}"), queue: [420, 440, 450][i % 3], started_at: at - 1800, ended_at: at, duration: 1800, remake: i == 1, participants, ..Default::default() }).unwrap();
    }
    app.store.remove_player("Player2#TEST").unwrap();
    for zone in ["UTC", "Asia/Ho_Chi_Minh", "America/New_York"] {
        app.cfg.reports.timezone = zone.into();
        let tz: chrono_tz::Tz = zone.parse().unwrap();
        let local_end = chrono::DateTime::from_timestamp(end, 0).unwrap().with_timezone(&tz);
        for period in [Period::Daily, Period::Weekly, Period::Monthly] {
            let start = match period {
                Period::Daily => end - 86400,
                Period::Weekly => end - 7 * 86400,
                Period::Monthly => tz.with_ymd_and_hms(local_end.year(), local_end.month(), 1, 0, 0, 0).earliest().unwrap().timestamp(),
            };
            let games = app.store.matches(start, end).unwrap();
            let expected = reports::compute(&app, period, start, end, &games).unwrap();
            let actual = reports::build_at(&app, period, end).unwrap();
            assert_eq!(serde_json::to_vec(&actual).unwrap(), serde_json::to_vec(&expected).unwrap(), "{zone} {}", period.key());
            if let (Some(actual), Some(expected)) = (actual, expected) {
                assert!(actual.players.iter().all(|p| p.name != "Nguyễn 2"));
                for lang in Lang::ALL { assert_eq!(actual.text(lang), expected.text(lang)); }
            }
        }
    }
    assert!(reports::build_at(&app, Period::Daily, end - 90 * 86400).unwrap().is_none());
}
