use crate::{i18n::{Lang, Msg}, model::{Match, Participant}};

pub fn limit(text: &str, max: usize) -> String {
    let mut units = 0;
    text.chars().take_while(|c| {
        units += c.len_utf16();
        units <= max
    }).collect()
}
// User-controlled names cannot break code blocks, inject markup or ping members.
pub fn plain(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() && !matches!(c, '`' | '*' | '_' | '~' | '|' | '@' | '<' | '>' | '\\')).take(64).collect()
}
pub fn outcome(p: &Participant, lang: Lang) -> &'static str { lang.t(if p.win { Msg::Win } else { Msg::Loss }) }
pub fn commentary(m: &Match, lang: Lang) -> String {
    if m.remake { return lang.t(Msg::RemakeExcluded).into(); }
    let mut lines = Vec::new();
    let minutes = (m.duration as f64 / 60.0).max(1.0);
    for p in m.participants.iter().filter(|p| p.tracked) {
        let team_kills: i64 = m.participants.iter().filter(|q| q.team == p.team).map(|q| q.kills).sum();
        let kp = ((p.kills + p.assists) as f64 / team_kills.max(1) as f64 * 100.0).min(100.0);
        let metric = if p.role == "UTILITY" {
            format!("{} · {}", lang.f(Msg::VisionPerMin, &[&format!("{:.1}", p.vision as f64 / minutes)]), lang.f(Msg::KillParticipation, &[&format!("{kp:.0}")]))
        } else {
            format!("{} · {}", lang.f(Msg::CsPerMin, &[&format!("{:.1}", p.cs as f64 / minutes)]), lang.f(Msg::DamageShort, &[&p.damage]))
        };
        lines.push(format!("{} — {} · {}/{}/{} · {metric}", plain(&p.name), outcome(p, lang), p.kills, p.deaths, p.assists));
    }
    if let Some(verdict) = verdict(m, lang) { lines.push(String::new()); lines.push(verdict); }
    limit(&lines.join("\n"), 1800)
}


// Role-aware performance score: each metric is compared with the lobby average and the lane
// opponent. Roughly 1.0 is an average performance. Index order matches `metrics`.
const KP: usize = 0; const KDA: usize = 1; const DMG: usize = 2; const TAKEN: usize = 3; const CS: usize = 4;
const VIS: usize = 5; const OBJ: usize = 6; const CC: usize = 7; const HELP: usize = 8; const EFF: usize = 9; const DEAD: usize = 10;
fn weights(role: &str) -> [f64; 11] {
    //          kp   kda  dmg  taken cs   vis  obj  cc   help eff  dead
    let mut w = [1.2, 1.0, 1.3, 0.4, 0.8, 0.4, 0.4, 0.3, 0.1, 0.5, 1.0];
    let overrides: &[(usize, f64)] = match role {
        "TOP" => &[(TAKEN, 0.8), (OBJ, 0.6)],
        "JUNGLE" => &[(KP, 1.5), (OBJ, 1.0), (VIS, 0.6), (CS, 0.5)],
        "MIDDLE" => &[(DMG, 1.5)],
        "BOTTOM" => &[(DMG, 1.6), (CS, 1.1), (TAKEN, 0.2)],
        "UTILITY" => &[(KP, 1.5), (DMG, 0.5), (CS, 0.0), (VIS, 1.4), (CC, 0.5), (HELP, 0.5), (EFF, 0.2)],
        _ => &[],
    };
    for (i, v) in overrides { w[*i] = *v; }
    w
}
fn metrics(p: &Participant, team: &[&Participant], minutes: f64) -> [f64; 11] {
    let sum = |f: fn(&Participant) -> i64| team.iter().map(|q| f(q)).sum::<i64>().max(1) as f64;
    [
        (p.kills + p.assists) as f64 / sum(|q| q.kills),
        (p.kills + p.assists) as f64 / p.deaths.max(1) as f64,
        p.damage as f64 / sum(|q| q.damage),
        p.damage_taken as f64 / minutes,
        p.cs as f64 / minutes,
        p.vision as f64 / minutes,
        p.objective_damage as f64 / minutes,
        p.cc_time as f64 / minutes,
        p.team_help as f64 / minutes,
        p.damage as f64 / p.gold.max(1) as f64,
        p.deaths as f64 / sum(|q| q.deaths),
    ]
}
fn scores(m: &Match, minutes: f64) -> Vec<f64> {
    let team = |id: u16| m.participants.iter().filter(|q| q.team == id).collect::<Vec<_>>();
    let all: Vec<[f64; 11]> = m.participants.iter().map(|p| metrics(p, &team(p.team), minutes)).collect();
    let mut avg = [0.0; 11];
    for k in 0..11 {
        avg[k] = all.iter().map(|v| v[k]).sum::<f64>() / all.len() as f64;
        if avg[k] == 0.0 { avg[k] = 1.0; }
    }
    m.participants.iter().zip(&all).map(|(p, own)| {
        let w = weights(&p.role);
        let opponent = m.participants.iter().zip(&all)
            .find(|(q, _)| !p.role.is_empty() && q.role == p.role && q.team != p.team).map_or(avg, |(_, v)| *v);
        let rel: Vec<f64> = (0..11).map(|k| {
            let base = (avg[k] + opponent[k]) / 2.0;
            (own[k] / if base == 0.0 { 1.0 } else { base }).min(2.0)
        }).collect();
        let good: f64 = (0..DEAD).map(|k| w[k] * rel[k]).sum();
        (good - w[DEAD] * (rel[DEAD] - 1.0)) / w[..DEAD].iter().sum::<f64>()
    }).collect()
}
/// The two most telling facts behind a verdict, from final statistics only.
fn reasons(p: &Participant, team: &[&Participant], minutes: f64, good: bool, lang: Lang) -> String {
    let v = metrics(p, team, minutes);
    let support = p.role == "UTILITY";
    let pct = |x: f64| format!("{:.0}%", x * 100.0);
    let one = |x: f64| format!("{x:.1}");
    let facts: Vec<(bool, String)> = if good { vec![
        (v[DMG] >= 0.3, lang.f(Msg::ReasonTeamDamage, &[&pct(v[DMG])])),
        (v[KP] >= 0.6, lang.f(Msg::ReasonKp, &[&pct(v[KP])])),
        (v[KDA] >= 4.0, format!("KDA {}", one(v[KDA]))),
        (v[VIS] >= 1.5, lang.f(Msg::VisionPerMin, &[&one(v[VIS])])),
        (v[OBJ] >= 400.0, lang.f(Msg::ReasonObjective, &[&(p.objective_damage / 1000)])),
        (!support && v[CS] >= 7.5, lang.f(Msg::CsPerMin, &[&one(v[CS])])),
        (v[TAKEN] >= 1400.0, lang.f(Msg::ReasonTaken, &[&(p.damage_taken / 1000)])),
        (p.deaths <= 1, lang.f(Msg::ReasonFewDeaths, &[&p.deaths])),
    ] } else { vec![
        (p.deaths >= 7, lang.f(Msg::ReasonDeaths, &[&p.deaths])),
        (!support && v[DMG] <= 0.12, lang.f(Msg::ReasonTeamDamage, &[&pct(v[DMG])])),
        (v[KP] <= 0.35, lang.f(Msg::ReasonKp, &[&pct(v[KP])])),
        (v[KDA] < 1.0, format!("KDA {}", one(v[KDA]))),
        (support && v[VIS] < 1.2, lang.f(Msg::VisionPerMin, &[&one(v[VIS])])),
        (!support && v[CS] < 5.0, lang.f(Msg::CsPerMin, &[&one(v[CS])])),
    ] };
    let hits: Vec<String> = facts.into_iter().filter(|(ok, _)| *ok).map(|(_, s)| s).take(2).collect();
    if !hits.is_empty() { return hits.join(", "); }
    lang.t(if good { Msg::ReasonSteady } else { Msg::ReasonLowest }).into()
}
/// Participant IDs of each team's standout player by the same score as `verdict` (5v5 only).
#[derive(Clone, Copy, PartialEq)]
pub enum Badge { Mvp, Worst }
/// Scoreboard badges (5v5 only), from the same score as the text verdict: each team's best player
/// (MVP on a win, ACE on a loss) and the lowest-scoring player of each team with tracked players.
pub fn badges(m: &Match) -> Vec<(u64, Badge)> {
    if m.remake || m.participants.len() != 10 { return Vec::new(); }
    let scores = scores(m, (m.duration as f64 / 60.0).max(1.0));
    let mut teams: Vec<u16> = m.participants.iter().map(|p| p.team).collect();
    teams.sort_unstable(); teams.dedup();
    let mut out = Vec::new();
    for team in teams {
        let mut rows: Vec<(&Participant, f64)> = m.participants.iter().zip(&scores).filter(|(p, _)| p.team == team).map(|(p, s)| (p, *s)).collect();
        rows.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (Some(&(worst, _)), Some(&(best, _))) = (rows.first(), rows.last()) else { continue };
        if best.win { out.push((best.id, Badge::Mvp)); }
        if rows.len() > 1 && rows.iter().any(|(p, _)| p.tracked) { out.push((worst.id, Badge::Worst)); }
    }
    out
}
/// Standout and least-impact player of each team containing tracked players (5v5 only).
pub fn verdict(m: &Match, lang: Lang) -> Option<String> {
    if m.remake || m.participants.len() != 10 { return None; }
    let minutes = (m.duration as f64 / 60.0).max(1.0);
    let scores = scores(m, minutes);
    let mut teams: Vec<u16> = m.participants.iter().filter(|p| p.tracked).map(|p| p.team).collect();
    teams.sort_unstable(); teams.dedup();
    let mut lines = Vec::new();
    for team in teams {
        let mut rows: Vec<(&Participant, f64)> = m.participants.iter().zip(&scores).filter(|(p, _)| p.team == team).map(|(p, s)| (p, *s)).collect();
        if rows.len() < 2 { continue; }
        rows.sort_by(|a, b| a.1.total_cmp(&b.1));
        let members: Vec<&Participant> = rows.iter().map(|(p, _)| *p).collect();
        let line = |icon: &str, label: Msg, p: &Participant, good: bool| format!("{icon} {}: {} ({} {}/{}/{}) — {}",
            lang.t(label), plain(&p.name), plain(&p.champion), p.kills, p.deaths, p.assists, reasons(p, &members, minutes, good, lang));
        lines.push(line("🏆", Msg::Standout, rows[rows.len() - 1].0, true));
        lines.push(line("📉", Msg::LeastImpact, rows[0].0, false));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}
