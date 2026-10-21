use crate::{App, model::{Match, Player, now}, reports};
use anyhow::{Result, bail};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc, time::Duration};
use tokio::time::{Instant, sleep};
use tracing::{info, warn};

/// A post-game rank is attributed to one match only if fetched this soon after it ended;
/// no second complete game fits inside this window.
const LP_WINDOW_SECS: i64 = 1200;
/// A restart this quick keeps the previous run's rank snapshots trusted (a deploy must not cost LP).
const CONTINUITY_SECS: i64 = 180;
/// Riot's league endpoint can trail a finished game by a minute or more, so an unchanged rank is
/// rechecked this many times, this far apart, before it is accepted as "no LP change".
const RANK_ATTEMPTS: u32 = 8;
const RANK_RETRY_SECS: u64 = 10;

pub struct Tracker {
    app: Arc<App>,
    /// Unix time since which tracking has been continuous. Only rank snapshots taken since then are
    /// trusted as a "before" rank, so downtime can never be folded into one match's LP change.
    since: i64,
    last_beat: i64,
    history_due: Instant,
    rank_due: Instant,
    prune_due: Instant,
}
impl Tracker {
    pub fn new(app: Arc<App>) -> Self {
        let now = Instant::now();
        // After a longer gap other games may sit between an old snapshot and the next game, so only newer snapshots count.
        // A quick restart inherits the stored continuity start; the heartbeat alone would only prove the last few seconds.
        let setting = |key: &str| app.store.setting(key).ok().flatten().and_then(|t| t.parse::<i64>().ok());
        let quick = setting("heartbeat").filter(|t| crate::model::now() - t <= CONTINUITY_SECS);
        let since = quick.map(|beat| setting("tracking_since").filter(|s| *s <= beat).unwrap_or(beat)).unwrap_or_else(crate::model::now);
        if let Err(error) = app.store.set("tracking_since", &since.to_string()) { warn!(%error, "Could not store the tracking start"); }
        Self { app, since, last_beat: 0, history_due: now, rank_due: now, prune_due: now }
    }
    pub async fn run(mut self, once: bool) -> Result<()> {
        if once { return self.tick().await; }
        loop {
            // Each step logs its own failure; the loop never stops on a transient error.
            let _ = self.tick().await;
            sleep(Duration::from_secs(5)).await;
        }
    }
    /// Runs every due step independently. Returns an error if any step failed.
    async fn tick(&mut self) -> Result<()> {
        let mut failed = false;
        let mut check = |step: &str, result: Result<()>| if let Err(error) = result { warn!(%error, step, "Tracking step failed; will retry"); failed = true; };
        // The heartbeat lets the next start tell a quick restart from real downtime.
        if now() - self.last_beat >= 30 {
            self.last_beat = now();
            check("heartbeat", self.app.store.set("heartbeat", &now().to_string()));
        }
        // Ranks first: a fresh snapshot must exist before any game it could serve as a baseline for.
        if Instant::now() >= self.rank_due {
            self.rank_due = Instant::now() + Duration::from_secs(1800);
            check("ranks", reports::refresh_ranks(&self.app).await.map(drop));
        }
        if Instant::now() >= self.history_due {
            self.history_due = Instant::now() + Duration::from_secs(self.app.cfg.riot.history_interval_secs);
            check("resolve", self.resolve_roster().await);
            check("history", self.history().await);
        }
        if Instant::now() >= self.prune_due {
            self.prune_due = Instant::now() + Duration::from_secs(86400);
            check("prune", self.app.store.prune(self.app.cfg.retention_days));
        }
        if failed { bail!("One or more tracking steps failed"); }
        Ok(())
    }
    async fn resolve_roster(&self) -> Result<()> {
        for mut player in self.app.store.players()? {
            if !player.puuid.is_empty() { continue; }
            match self.app.riot.resolve(&player).await {
                Ok(Some(puuid)) => {
                    // Do not permit one account under multiple aliases.
                    if self.app.store.players()?.iter().any(|p| p.puuid == puuid) {
                        warn!("Roster contains duplicate account; unresolved entry skipped"); continue;
                    }
                    player.puuid = puuid;
                    self.app.store.update_player(&player)?;
                }
                Ok(None) => warn!("One Riot ID could not be resolved; check the private roster"),
                Err(error) => warn!(%error, "Account resolution failed"),
            }
        }
        Ok(())
    }
    async fn history(&self) -> Result<()> {
        let roster = self.app.store.players()?;
        // Build candidates across the full roster, then sort by end time before rank enrichment.
        let mut candidates = HashSet::new();
        for player in roster.iter().filter(|p| !p.puuid.is_empty()) {
            let ids = match self.app.riot.history(&player.puuid, self.app.cfg.riot.history_count).await {
                Ok(ids) => ids,
                Err(error) => { warn!(%error, "History lookup failed"); continue; }
            };
            if !player.baselined {
                self.app.store.baseline(player, &ids)?;
                info!("Initialized one player's history baseline without replaying old games");
                continue;
            }
            for id in ids {
                if !self.app.store.skip(&player.puuid, &id)? { candidates.insert(id); }
            }
        }
        let mut matches: Vec<(Match, Value)> = Vec::new();
        for id in candidates {
            match self.app.riot.match_data(&id).await {
                Ok(Some(value)) => match Match::from_riot(&id, &value, &roster) {
                    Ok(m) => matches.push((m, value)),
                    // Permanent for this payload: never refetch it on every sweep.
                    Err(error) => { warn!(%error, "Match normalization failed; skipping it"); self.app.store.mark_seen(&id)?; }
                },
                Ok(None) => (),
                Err(error) => warn!(%error, "Match retrieval failed"),
            }
        }
        matches.sort_by_key(|(m, _)| m.ended_at);
        for (m, raw) in matches {
            if now() - m.ended_at > i64::from(self.app.cfg.riot.catch_up_hours) * 3600 {
                self.app.store.mark_seen(&m.id)?;
                continue;
            }
            if let Err(error) = self.finish(m.clone()).await { warn!(%error, "Storing a finished match failed; will retry"); }
            // The untouched payload is the long-term record; keep it only for matches the store accepted.
            else if let Err(error) = self.app.store.save_raw(&m, &raw) { warn!(%error, "Storing the raw match payload failed"); }
        }
        Ok(())
    }
    async fn finish(&self, mut m: Match) -> Result<()> {
        if self.app.store.seen(&m.id)? { return Ok(()); }
        let current: HashSet<String> = self.app.store.players()?.into_iter().map(|p| p.puuid).collect();
        for part in &mut m.participants {
            part.tracked &= current.contains(&part.puuid);
            if !part.tracked || !matches!(m.queue, 420 | 440) { continue; }
            // The newest snapshot taken at or before the start by this process. Older snapshots
            // could span downtime with other games.
            part.rank_before = self.app.store.rank_at(&part.puuid, m.queue, m.started_at).ok().flatten()
                .filter(|s| s.at >= self.since).map(|s| s.rank);
            let mut after = None;
            for attempt in 0..RANK_ATTEMPTS {
                match self.app.riot.ranks(&part.puuid).await {
                    Ok(ranks) => {
                        for rank in &ranks { self.app.store.snapshot(&part.puuid, rank, now())?; }
                        after = ranks.into_iter().find(|r| r.queue == m.queue);
                    }
                    Err(error) => { warn!(%error, "Post-game rank unavailable; preserving match result"); break; }
                }
                if part.rank_before.is_none() || after != part.rank_before || m.remake || attempt + 1 == RANK_ATTEMPTS { break; }
                sleep(Duration::from_secs(RANK_RETRY_SECS)).await;
            }
            // A post-game lookup long after the match can include later games.
            if now() - m.ended_at <= LP_WINDOW_SECS {
                part.lp_delta = part.rank_before.as_ref().and_then(|r| r.absolute()).zip(after.as_ref().and_then(|r| r.absolute()))
                    .map(|(a, b)| b - a).filter(|delta| *delta != 0 || m.remake);
            }
            part.rank_after = after;
        }
        if m.participants.iter().any(|p| p.tracked) {
            // Everyone else's rank for the scoreboard. A failure (usually the rate limit) only drops the remaining ranks.
            if matches!(m.queue, 420 | 440) && !m.remake {
                for part in m.participants.iter_mut().filter(|p| !p.tracked && !p.puuid.is_empty()) {
                    match self.app.riot.ranks(&part.puuid).await {
                        Ok(ranks) => part.rank_after = ranks.into_iter().find(|r| r.queue == m.queue),
                        Err(error) => { warn!(%error, "Rank of another player unavailable"); break; }
                    }
                }
            }
            self.app.store.finish(&m)?;
            info!("Stored a finished tracked match");
        }
        else { self.app.store.discard(&m.id)?; }
        Ok(())
    }
}

pub fn parse_player(id: &str, nickname: &str) -> Result<Player> {
    let (name, tag) = id.trim().rsplit_once('#').ok_or_else(|| anyhow::anyhow!("Riot ID must be Name#Tag"))?;
    let player = Player { game_name: name.trim().into(), tag_line: tag.trim().into(), nickname: nickname.trim().into(), puuid: String::new(), baselined: false };
    player.validate()?;
    Ok(player)
}
