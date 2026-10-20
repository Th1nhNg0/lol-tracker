use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn now() -> i64 { chrono::Utc::now().timestamp() }

#[derive(Clone, Serialize, Deserialize)]
pub struct Player {
    pub game_name: String,
    pub tag_line: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub puuid: String,
    #[serde(default)]
    pub baselined: bool,
}
impl Player {
    pub fn riot_id(&self) -> String { format!("{}#{}", self.game_name, self.tag_line) }
    pub fn name(&self) -> &str { if self.nickname.is_empty() { &self.game_name } else { &self.nickname } }
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.game_name.trim().is_empty() && !self.tag_line.trim().is_empty(), "Riot ID must be Name#Tag");
        ensure!(self.game_name.chars().count() <= 64 && self.tag_line.chars().count() <= 16 && self.nickname.chars().count() <= 64, "Player fields too long");
        ensure!(!self.game_name.contains(['\n', '\r', '#']) && !self.tag_line.contains(['\n', '\r', '#']), "Invalid Riot ID");
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Rank {
    pub queue: u16,
    pub tier: String,
    pub division: String,
    pub lp: i64,
}
impl Rank {
    pub fn absolute(&self) -> Option<i64> {
        let base = match self.tier.as_str() {
            "IRON" => 0, "BRONZE" => 400, "SILVER" => 800, "GOLD" => 1200,
            "PLATINUM" => 1600, "EMERALD" => 2000, "DIAMOND" => 2400,
            "MASTER" | "GRANDMASTER" | "CHALLENGER" => return Some(2800 + self.lp),
            _ => return None,
        };
        let offset = match self.division.as_str() { "IV" => 0, "III" => 100, "II" => 200, "I" => 300, _ => return None };
        Some(base + offset + self.lp)
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot { pub at: i64, pub rank: Rank }

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Participant {
    pub id: u64,
    pub puuid: String,
    pub name: String,
    pub champion: String,
    pub team: u16,
    pub role: String,
    pub tracked: bool,
    pub win: bool,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub cs: i64,
    pub damage: i64,
    pub gold: i64,
    pub vision: i64,
    #[serde(default)]
    pub level: i64,
    #[serde(default)]
    pub damage_taken: i64,
    #[serde(default)]
    pub objective_damage: i64,
    #[serde(default)]
    pub cc_time: i64,
    /// Healing and shielding on teammates.
    #[serde(default)]
    pub team_help: i64,
    pub items: Vec<i64>,
    pub rank_before: Option<Rank>,
    pub rank_after: Option<Rank>,
    pub lp_delta: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Match {
    pub id: String,
    pub queue: u16,
    pub started_at: i64,
    pub ended_at: i64,
    pub duration: i64,
    pub remake: bool,
    pub participants: Vec<Participant>,
    pub objectives: BTreeMap<String, BTreeMap<String, i64>>,
}
impl Match {
    pub fn from_riot(id: &str, value: &Value, roster: &[Player]) -> Result<Self> {
        let info = &value["info"];
        let rows = info["participants"].as_array().ok_or_else(|| anyhow::anyhow!("Match is missing participants"))?;
        ensure!(!rows.is_empty() && rows.len() <= 32, "Invalid participant count");
        let started_at = n(info, "gameStartTimestamp") / 1000;
        let duration = n(info, "gameDuration");
        let ended_at = match n(info, "gameEndTimestamp") { 0 => started_at + duration, ts => ts / 1000 };
        ensure!(started_at > 0 && duration > 0 && ended_at >= started_at, "Invalid match timestamps");
        let participants = rows.iter().map(|v| {
            let puuid = s(v, "puuid");
            let player = roster.iter().find(|p| !p.puuid.is_empty() && p.puuid == puuid);
            Participant {
                id: n(v, "participantId") as u64, puuid,
                name: player.map(|p| p.name().to_owned()).unwrap_or_else(|| {
                    let name = s(v, "riotIdGameName"); if name.is_empty() { "Unknown".into() } else { name }
                }),
                champion: s(v, "championName"), team: n(v, "teamId") as u16, role: s(v, "teamPosition"),
                tracked: player.is_some(), win: v["win"].as_bool().unwrap_or(false),
                kills: n(v, "kills"), deaths: n(v, "deaths"), assists: n(v, "assists"),
                cs: n(v, "totalMinionsKilled") + n(v, "neutralMinionsKilled"), damage: n(v, "totalDamageDealtToChampions"),
                gold: n(v, "goldEarned"), vision: n(v, "visionScore"),
                level: n(v, "champLevel"), damage_taken: n(v, "totalDamageTaken"),
                objective_damage: n(v, "damageDealtToObjectives"), cc_time: n(v, "timeCCingOthers"),
                team_help: n(v, "totalHealsOnTeammates") + n(v, "totalDamageShieldedOnTeammates"),
                items: (0..7).map(|i| n(v, &format!("item{i}"))).collect(),
                ..Default::default()
            }
        }).collect();
        let mut objectives = BTreeMap::new();
        for team in info["teams"].as_array().into_iter().flatten() {
            let values = team["objectives"].as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), n(v, "kills"))).collect()).unwrap_or_default();
            objectives.insert(n(team, "teamId").to_string(), values);
        }
        Ok(Self { id: id.to_owned(), queue: n(info, "queueId") as u16, started_at, ended_at, duration,
            remake: rows.iter().any(|v| v["gameEndedInEarlySurrender"].as_bool().unwrap_or(false)), participants, objectives })
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub enum Notice {
    Result(Match),
    /// `report` is absent in notices queued by older versions; they are sent as text.
    Report { text: String, #[serde(default)] report: Option<crate::reports::Report> },
}
pub fn n(v: &Value, key: &str) -> i64 { v[key].as_i64().unwrap_or(0) }
pub fn s(v: &Value, key: &str) -> String { v[key].as_str().unwrap_or("").to_owned() }
