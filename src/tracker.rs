use crate::{App, model::{Match, Player, now}, reports};
use anyhow::{Result, bail};
use std::{collections::HashSet, sync::Arc, time::Duration};
use tokio::time::{Instant, sleep};
use tracing::{info, warn};

/// A post-game rank is attributed to one match only if fetched this soon after it ended;
/// no second complete game fits inside this window.
const LP_WINDOW_SECS: i64 = 1200;

pub struct Tracker {
    app: Arc<App>,
    /// Unix time this process started tracking. Only rank snapshots taken since then are trusted
    /// as a "before" rank, so downtime can never be folded into one match's LP change.
    since: i64,
    history_due: Instant,
    rank_due: Instant,
    prune_due: Instant,
}
impl Tracker {
    pub fn new(app: Arc<App>) -> Self {
        let now = Instant::now();
        Self { app, since: crate::model::now(), history_due: now, rank_due: now, prune_due: now }
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
        let mut matches = Vec::new();
        for id in candidates {
            match self.app.riot.match_data(&id).await {
                Ok(Some(value)) => match Match::from_riot(&id, &value, &roster) {
                    Ok(m) => matches.push(m),
                    // Permanent for this payload: never refetch it on every sweep.
                    Err(error) => { warn!(%error, "Match normalization failed; skipping it"); self.app.store.mark_seen(&id)?; }
                },
                Ok(None) => (),
                Err(error) => warn!(%error, "Match retrieval failed"),
            }
        }
        matches.sort_by_key(|m| m.ended_at);
        for m in matches {
            if now() - m.ended_at > i64::from(self.app.cfg.riot.catch_up_hours) * 3600 {
                self.app.store.mark_seen(&m.id)?;
                continue;
            }
            if let Err(error) = self.finish(m).await { warn!(%error, "Storing a finished match failed; will retry"); }
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
            for attempt in 0..3 {
                match self.app.riot.ranks(&part.puuid).await {
                    Ok(ranks) => {
                        for rank in &ranks { self.app.store.snapshot(&part.puuid, rank, now())?; }
                        after = ranks.into_iter().find(|r| r.queue == m.queue);
                    }
                    Err(error) => { warn!(%error, "Post-game rank unavailable; preserving match result"); break; }
                }
                if part.rank_before.is_none() || after != part.rank_before || m.remake || attempt == 2 { break; }
                sleep(Duration::from_secs(5)).await;
            }
            // A post-game lookup long after the match can include later games.
            if now() - m.ended_at <= LP_WINDOW_SECS {
                part.lp_delta = part.rank_before.as_ref().and_then(|r| r.absolute()).zip(after.as_ref().and_then(|r| r.absolute()))
                    .map(|(a, b)| b - a).filter(|delta| *delta != 0 || m.remake);
            }
            part.rank_after = after;
        }
        if m.participants.iter().any(|p| p.tracked) { self.app.store.finish(&m)?; info!("Stored a finished tracked match"); }
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
